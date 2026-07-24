use std::{
    any::Any,
    ffi::{CStr, CString},
    io::{Read, Write},
    net::{IpAddr, Shutdown, TcpListener, TcpStream, ToSocketAddrs, UdpSocket},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use crate::{EngineParams, NetError, Result, dns, error::Error, sys};

/// A started Cronet engine.
///
/// Requests borrow their engine, so Rust prevents normal destruction while a
/// request wrapper is alive.
pub struct Engine {
    raw: NonNull<sys::Cronet_Engine>,
    hooks: Option<Box<NetworkHookState>>,
}

// SAFETY: Cronet's Engine API is designed for calls from application threads;
// ownership remains unique and this wrapper does not expose unsynchronized
// interior Rust state.
unsafe impl Send for Engine {}
// SAFETY: Cronet Engine methods and SagerNet socket hooks are thread-safe; hook
// closures require Sync and all Rust-owned state is immutable after startup.
unsafe impl Sync for Engine {}

impl Engine {
    /// Creates and starts an engine using `params`.
    pub fn start(params: &EngineParams) -> Result<Self> {
        // SAFETY: Native constructor takes no arguments.
        let ptr = unsafe { sys::Cronet_Engine_Create() };
        let ptr = NonNull::new(ptr).expect("Cronet_Engine_Create returned null");
        // SAFETY: Both objects are live for the duration of the call.
        let result = unsafe { sys::Cronet_Engine_StartWithParams(ptr.as_ptr(), params.as_ptr()) };
        if let Err(error) = Error::from_result(result) {
            // SAFETY: Startup failed and this wrapper uniquely owns the engine.
            unsafe { sys::Cronet_Engine_Destroy(ptr.as_ptr()) };
            return Err(error);
        }
        Ok(Self {
            raw: ptr,
            hooks: None,
        })
    }

    /// Creates an engine with the custom socket hooks used by SagerNet Cronet.
    pub fn start_with_network_hooks(params: &EngineParams, hooks: NetworkHooks) -> Result<Self> {
        // SAFETY: Native constructor takes no arguments.
        let raw = unsafe { sys::Cronet_Engine_Create() };
        let raw = NonNull::new(raw).expect("Cronet_Engine_Create returned null");
        let mut state = Box::new(NetworkHookState {
            tcp: hooks.tcp,
            udp: hooks.udp,
            _keepalive: hooks._keepalive,
        });
        let context = (&mut *state as *mut NetworkHookState).cast();

        // SAFETY: Callback context is heap-stable and retained by Engine.
        unsafe {
            sys::Cronet_Engine_SetDialer(
                raw.as_ptr(),
                state.tcp.as_ref().map(|_| tcp_dialer_trampoline as _),
                context,
            );
            sys::Cronet_Engine_SetUdpDialer(
                raw.as_ptr(),
                state.udp.as_ref().map(|_| udp_dialer_trampoline as _),
                context,
            );
        }

        if let Some(pem) = hooks.trusted_root_certificates {
            let pem = CString::new(pem).expect("PEM certificates cannot contain NUL bytes");
            // SAFETY: Engine and PEM are live for the call. SagerNet Cronet
            // transfers ownership of the returned verifier to the engine.
            let verifier = unsafe { sys::Cronet_CreateCertVerifierWithRootCerts(pem.as_ptr()) };
            if verifier.is_null() {
                // SAFETY: Startup has not occurred and engine is uniquely owned.
                unsafe { sys::Cronet_Engine_Destroy(raw.as_ptr()) };
                return Error::from_result(sys::Cronet_RESULT_Cronet_RESULT_ILLEGAL_ARGUMENT)
                    .map(|()| unreachable!());
            }
            // SAFETY: Verifier ownership is transferred to the engine.
            unsafe {
                sys::Cronet_Engine_SetMockCertVerifierForTesting(raw.as_ptr(), verifier);
            }
        }

        // SAFETY: Engine and params are live.
        let result = unsafe { sys::Cronet_Engine_StartWithParams(raw.as_ptr(), params.as_ptr()) };
        if let Err(error) = Error::from_result(result) {
            // SAFETY: Startup failed and engine is uniquely owned.
            unsafe { sys::Cronet_Engine_Destroy(raw.as_ptr()) };
            return Err(error);
        }
        Ok(Self {
            raw,
            hooks: Some(state),
        })
    }

    /// Returns the native Cronet version string.
    pub fn version(&self) -> &str {
        // SAFETY: Engine is live and Cronet owns a NUL-terminated return value.
        unsafe { CStr::from_ptr(sys::Cronet_Engine_GetVersionString(self.raw.as_ptr())) }
            .to_str()
            .unwrap_or("<non-UTF-8 Cronet version>")
    }

    /// Returns Cronet's default user agent.
    pub fn default_user_agent(&self) -> &str {
        // SAFETY: Engine is live and Cronet owns a NUL-terminated return value.
        unsafe { CStr::from_ptr(sys::Cronet_Engine_GetDefaultUserAgent(self.raw.as_ptr())) }
            .to_str()
            .unwrap_or("<non-UTF-8 Cronet user agent>")
    }

    /// Starts writing NetLog events to `path`.
    pub fn start_net_log(&self, path: &str, log_all: bool) -> bool {
        let path = CString::new(path).expect("NetLog path cannot contain NUL bytes");
        // SAFETY: Engine and path are valid for the duration of the call.
        unsafe { sys::Cronet_Engine_StartNetLogToFile(self.raw.as_ptr(), path.as_ptr(), log_all) }
    }

    /// Stops an active NetLog session.
    pub fn stop_net_log(&self) {
        // SAFETY: Engine is live.
        unsafe { sys::Cronet_Engine_StopNetLog(self.raw.as_ptr()) };
    }

    /// Returns the native pointer for advanced generated-ABI calls.
    pub fn as_raw(&self) -> sys::Cronet_EnginePtr {
        self.raw.as_ptr()
    }

    /// Closes all pooled H2/H3/TCP connections in SagerNet Cronet.
    pub fn close_all_connections(&self) {
        // SAFETY: Engine is live.
        unsafe { sys::Cronet_Engine_CloseAllConnections(self.raw.as_ptr()) };
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: Safe requests borrow the engine. Cronet documents Shutdown
        // followed by Destroy as the required ownership sequence.
        unsafe {
            let _ = sys::Cronet_Engine_Shutdown(self.raw.as_ptr());
            sys::Cronet_Engine_Destroy(self.raw.as_ptr());
        }
        drop(self.hooks.take());
    }
}

/// Custom socket hooks provided by the SagerNet Cronet fork.
#[derive(Default)]
pub struct NetworkHooks {
    tcp: Option<Box<TcpDialer>>,
    udp: Option<Box<UdpDialer>>,
    trusted_root_certificates: Option<String>,
    dns_resolver: Option<Arc<RwLock<Arc<dns::Resolver>>>>,
    _keepalive: Vec<Box<dyn Any + Send + Sync>>,
}

type TcpDialer = dyn Fn(&str, u16) -> i32 + Send + Sync;
type UdpDialer = dyn Fn(&str, u16) -> UdpDialResult + Send + Sync;
type CloseCallback = dyn Fn() + Send + Sync;
type DatagramReceiver = dyn Fn(&mut [u8]) -> std::io::Result<usize> + Send + Sync;
type DatagramSender = dyn Fn(&[u8]) -> std::io::Result<usize> + Send + Sync;

/// Independently owned read and write halves of a custom byte stream.
///
/// The halves are driven concurrently by Cronet bridge threads. Call
/// [`Self::on_close`] for transports that need an explicit cancellation signal
/// to unblock a pending read when the other direction closes.
pub struct SplitStream {
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    close: Option<Arc<CloseCallback>>,
}

impl SplitStream {
    /// Creates a split stream from independently owned read and write halves.
    pub fn new(reader: impl Read + Send + 'static, writer: impl Write + Send + 'static) -> Self {
        Self {
            reader: Box::new(reader),
            writer: Box::new(writer),
            close: None,
        }
    }

    /// Installs a callback invoked once when either bridge direction ends.
    pub fn on_close(mut self, close: impl Fn() + Send + Sync + 'static) -> Self {
        self.close = Some(Arc::new(close));
        self
    }
}

/// Callback-backed packet transport used by [`NetworkHooks::udp_datagram_connector`].
///
/// Each receive or send callback handles exactly one complete datagram. The
/// reported local endpoint is passed to Cronet for QUIC socket bookkeeping.
pub struct SplitDatagram {
    receive: Arc<DatagramReceiver>,
    send: Arc<DatagramSender>,
    local_address: String,
    local_port: u16,
    close: Option<Arc<CloseCallback>>,
}

impl SplitDatagram {
    /// Creates a datagram transport with its Cronet-visible local endpoint.
    pub fn new(
        receive: impl Fn(&mut [u8]) -> std::io::Result<usize> + Send + Sync + 'static,
        send: impl Fn(&[u8]) -> std::io::Result<usize> + Send + Sync + 'static,
        local_address: impl Into<String>,
        local_port: u16,
    ) -> Self {
        Self {
            receive: Arc::new(receive),
            send: Arc::new(send),
            local_address: local_address.into(),
            local_port,
            close: None,
        }
    }

    /// Installs a callback invoked once when either bridge direction ends.
    pub fn on_close(mut self, close: impl Fn() + Send + Sync + 'static) -> Self {
        self.close = Some(Arc::new(close));
        self
    }

    /// Returns the local IP literal reported to Cronet.
    pub fn local_address(&self) -> &str {
        &self.local_address
    }

    /// Returns the local port reported to Cronet.
    pub fn local_port(&self) -> u16 {
        self.local_port
    }
}

impl NetworkHooks {
    /// Creates empty hooks that use Cronet's normal socket stack.
    pub fn new() -> Self {
        Self::default()
    }

    /// Redirects TCP connection establishment.
    pub fn tcp_dialer(mut self, dialer: impl Fn(&str, u16) -> i32 + Send + Sync + 'static) -> Self {
        self.tcp = Some(Box::new(dialer));
        self
    }

    /// Transfers connected TCP sockets returned by `connector` to Cronet.
    ///
    /// Ownership of a successful socket is consumed by the native network
    /// stack. I/O errors are mapped to Chromium `net::Error` values.
    pub fn tcp_connector(
        mut self,
        connector: impl Fn(&str, u16) -> std::io::Result<TcpStream> + Send + Sync + 'static,
    ) -> Self {
        self.tcp = Some(Box::new(move |address, port| {
            connector(address, port)
                .and_then(tcp_stream_into_cronet_fd)
                .unwrap_or_else(|error| NetError::from_io(&error).code())
        }));
        self
    }

    /// Bridges an arbitrary split byte stream into a socket owned by Cronet.
    ///
    /// This is the Rust equivalent of `cronet-go`'s generic `net.Conn`
    /// fallback and supports SOCKS, encrypted, multiplexed or in-memory
    /// transports whose read and write halves can run concurrently.
    pub fn tcp_stream_connector(
        mut self,
        connector: impl Fn(&str, u16) -> std::io::Result<SplitStream> + Send + Sync + 'static,
    ) -> Self {
        self.tcp = Some(Box::new(move |address, port| {
            let stream = match connector(address, port) {
                Ok(stream) => stream,
                Err(error) => return NetError::from_io(&error).code(),
            };
            let (cronet, proxy) = match tcp_loopback_pair() {
                Ok(pair) => pair,
                Err(error) => return NetError::from_io(&error).code(),
            };
            let fd = match tcp_stream_into_cronet_fd(cronet) {
                Ok(fd) => fd,
                Err(error) => return NetError::from_io(&error).code(),
            };
            bridge_split_stream(stream, proxy);
            fd
        }));
        self
    }

    /// Redirects UDP socket creation.
    pub fn udp_dialer(
        mut self,
        dialer: impl Fn(&str, u16) -> UdpDialResult + Send + Sync + 'static,
    ) -> Self {
        self.udp = Some(Box::new(dialer));
        self
    }

    /// Transfers connected UDP sockets returned by `connector` to Cronet.
    pub fn udp_connector(
        mut self,
        connector: impl Fn(&str, u16) -> std::io::Result<UdpSocket> + Send + Sync + 'static,
    ) -> Self {
        self.udp = Some(Box::new(move |address, port| {
            let socket = match connector(address, port) {
                Ok(socket) => socket,
                Err(error) => {
                    return UdpDialResult {
                        fd: NetError::from_io(&error).code(),
                        local_address: String::new(),
                        local_port: 0,
                    };
                }
            };
            let (local_address, local_port) = socket
                .local_addr()
                .map(|address| (address.ip().to_string(), address.port()))
                .unwrap_or_default();
            let fd = udp_socket_into_cronet_fd(socket)
                .unwrap_or_else(|error| NetError::from_io(&error).code());
            UdpDialResult {
                fd,
                local_address,
                local_port,
            }
        }));
        self
    }

    /// Bridges an arbitrary datagram transport into a UDP socket owned by
    /// Cronet while preserving packet boundaries.
    pub fn udp_datagram_connector(
        mut self,
        connector: impl Fn(&str, u16) -> std::io::Result<SplitDatagram> + Send + Sync + 'static,
    ) -> Self {
        let shutdown = Arc::new(AtomicBool::new(false));
        let bridge_shutdown = Arc::clone(&shutdown);
        self.udp = Some(Box::new(move |address, port| {
            let datagram = match connector(address, port) {
                Ok(datagram) => datagram,
                Err(error) => return udp_error(&error),
            };
            let (cronet, proxy) = match udp_loopback_pair() {
                Ok(pair) => pair,
                Err(error) => return udp_error(&error),
            };
            let fd = match udp_socket_into_cronet_fd(cronet) {
                Ok(fd) => fd,
                Err(error) => return udp_error(&error),
            };
            let result = UdpDialResult {
                fd,
                local_address: datagram.local_address.clone(),
                local_port: datagram.local_port,
            };
            bridge_split_datagram(datagram, proxy, Arc::clone(&bridge_shutdown));
            result
        }));
        self._keepalive
            .push(Box::new(BridgeShutdownGuard(shutdown)));
        self
    }

    /// Intercepts Cronet's DNS-over-UDP and DNS-over-TCP traffic.
    ///
    /// `resolver` receives one unframed DNS wire message and must return one
    /// unframed DNS response. Existing TCP/UDP dialers remain the fallback for
    /// non-DNS destinations.
    pub fn dns_resolver(
        mut self,
        resolver: impl Fn(&[u8]) -> std::io::Result<Vec<u8>> + Send + Sync + 'static,
    ) -> Self {
        let resolver: Arc<dns::Resolver> = Arc::new(resolver);
        let resolver = Arc::new(RwLock::new(resolver));
        let shutdown = Arc::new(AtomicBool::new(false));
        let previous_tcp = self.tcp.take();
        let previous_udp = self.udp.take();

        let tcp_resolver = Arc::clone(&resolver);
        let tcp_shutdown = Arc::clone(&shutdown);
        self.tcp = Some(Box::new(move |address, port| {
            if is_dns_endpoint(address, port) {
                return dns::tcp_proxy(current_resolver(&tcp_resolver), Arc::clone(&tcp_shutdown))
                    .and_then(tcp_stream_into_cronet_fd)
                    .unwrap_or_else(|error| NetError::from_io(&error).code());
            }
            previous_tcp.as_ref().map_or_else(
                || {
                    system_tcp_connect(address, port)
                        .and_then(tcp_stream_into_cronet_fd)
                        .unwrap_or_else(|error| NetError::from_io(&error).code())
                },
                |dialer| dialer(address, port),
            )
        }));

        let udp_resolver = Arc::clone(&resolver);
        let udp_shutdown = Arc::clone(&shutdown);
        self.udp = Some(Box::new(move |address, port| {
            if is_dns_endpoint(address, port) {
                return match dns::udp_proxy(
                    current_resolver(&udp_resolver),
                    Arc::clone(&udp_shutdown),
                ) {
                    Ok(socket) => {
                        let (local_address, local_port) = socket
                            .local_addr()
                            .map(|address| (address.ip().to_string(), address.port()))
                            .unwrap_or_default();
                        UdpDialResult {
                            fd: udp_socket_into_cronet_fd(socket)
                                .unwrap_or_else(|error| NetError::from_io(&error).code()),
                            local_address,
                            local_port,
                        }
                    }
                    Err(error) => UdpDialResult {
                        fd: NetError::from_io(&error).code(),
                        local_address: String::new(),
                        local_port: 0,
                    },
                };
            }
            previous_udp.as_ref().map_or_else(
                || system_udp_connect(address, port),
                |dialer| dialer(address, port),
            )
        }));
        self.dns_resolver = Some(resolver);
        self._keepalive.push(Box::new(dns::ShutdownGuard(shutdown)));
        self
    }

    pub(crate) fn has_dns_resolver(&self) -> bool {
        self.dns_resolver.is_some()
    }

    pub(crate) fn configure_ech(&mut self, options: dns::EchOptions) {
        let Some(resolver) = &self.dns_resolver else {
            return;
        };
        let base = current_resolver(resolver);
        match resolver.write() {
            Ok(mut resolver) => *resolver = dns::with_ech(base, options),
            Err(poisoned) => *poisoned.into_inner() = dns::with_ech(base, options),
        }
    }

    pub(crate) fn configure_server_redirect(
        &mut self,
        server_name: String,
        server_address: String,
    ) {
        let Some(resolver) = &self.dns_resolver else {
            return;
        };
        let base = current_resolver(resolver);
        match resolver.write() {
            Ok(mut resolver) => {
                *resolver = dns::with_server_redirect(base, server_name, server_address);
            }
            Err(poisoned) => {
                *poisoned.into_inner() =
                    dns::with_server_redirect(base, server_name, server_address);
            }
        }
    }

    /// Replaces trusted root certificates with PEM-encoded roots.
    pub fn trusted_root_certificates(mut self, pem: impl Into<String>) -> Self {
        self.trusted_root_certificates = Some(pem.into());
        self
    }
}

fn current_resolver(resolver: &RwLock<Arc<dns::Resolver>>) -> Arc<dns::Resolver> {
    match resolver.read() {
        Ok(resolver) => Arc::clone(&resolver),
        Err(poisoned) => Arc::clone(&poisoned.into_inner()),
    }
}

fn system_tcp_connect(address: &str, port: u16) -> std::io::Result<TcpStream> {
    TcpStream::connect((address, port))
}

fn system_udp_connect(address: &str, port: u16) -> UdpDialResult {
    let outcome = (address, port).to_socket_addrs().and_then(|addresses| {
        let mut last_error = None;
        for remote in addresses {
            let bind_address = match remote.ip() {
                IpAddr::V4(_) => "0.0.0.0:0",
                IpAddr::V6(_) => "[::]:0",
            };
            match UdpSocket::bind(bind_address).and_then(|socket| {
                socket.connect(remote)?;
                Ok(socket)
            }) {
                Ok(socket) => return Ok(socket),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "no UDP destination addresses")
        }))
    });
    match outcome {
        Ok(socket) => {
            let (local_address, local_port) = socket
                .local_addr()
                .map(|address| (address.ip().to_string(), address.port()))
                .unwrap_or_default();
            UdpDialResult {
                fd: udp_socket_into_cronet_fd(socket)
                    .unwrap_or_else(|error| NetError::from_io(&error).code()),
                local_address,
                local_port,
            }
        }
        Err(error) => UdpDialResult {
            fd: NetError::from_io(&error).code(),
            local_address: String::new(),
            local_port: 0,
        },
    }
}

fn udp_error(error: &std::io::Error) -> UdpDialResult {
    UdpDialResult {
        fd: NetError::from_io(error).code(),
        local_address: String::new(),
        local_port: 0,
    }
}

fn tcp_loopback_pair() -> std::io::Result<(TcpStream, TcpStream)> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let client = TcpStream::connect(listener.local_addr()?)?;
    let (server, _) = listener.accept()?;
    Ok((client, server))
}

fn udp_loopback_pair() -> std::io::Result<(UdpSocket, UdpSocket)> {
    let first = UdpSocket::bind(("127.0.0.1", 0))?;
    let second = UdpSocket::bind(("127.0.0.1", 0))?;
    first.connect(second.local_addr()?)?;
    second.connect(first.local_addr()?)?;
    Ok((first, second))
}

fn bridge_split_stream(stream: SplitStream, proxy: TcpStream) {
    let SplitStream {
        mut reader,
        mut writer,
        close,
    } = stream;
    let proxy_reader = match proxy.try_clone() {
        Ok(socket) => socket,
        Err(_) => {
            if let Some(close) = close {
                close();
            }
            return;
        }
    };
    let shutdown_reader = proxy.try_clone().ok();
    let shutdown_writer = proxy.try_clone().ok();
    let closed = Arc::new(AtomicBool::new(false));
    let finish: Arc<CloseCallback> = Arc::new(move || {
        if !closed.swap(true, Ordering::AcqRel) {
            if let Some(socket) = &shutdown_reader {
                let _ = socket.shutdown(Shutdown::Both);
            }
            if let Some(socket) = &shutdown_writer {
                let _ = socket.shutdown(Shutdown::Both);
            }
            if let Some(close) = &close {
                close();
            }
        }
    });

    let reader_finish = Arc::clone(&finish);
    thread::spawn(move || {
        let mut proxy = proxy;
        let _ = std::io::copy(&mut reader, &mut proxy);
        let _ = proxy.shutdown(Shutdown::Both);
        reader_finish();
    });

    thread::spawn(move || {
        let mut proxy_reader = proxy_reader;
        let _ = std::io::copy(&mut proxy_reader, &mut writer);
        let _ = writer.flush();
        finish();
    });
}

fn bridge_split_datagram(
    datagram: SplitDatagram,
    proxy: UdpSocket,
    engine_shutdown: Arc<AtomicBool>,
) {
    let SplitDatagram {
        receive,
        send,
        close,
        ..
    } = datagram;
    let proxy_reader = match proxy.try_clone() {
        Ok(socket) => socket,
        Err(_) => {
            if let Some(close) = close {
                close();
            }
            return;
        }
    };
    let _ = proxy.set_read_timeout(Some(Duration::from_millis(100)));
    let _ = proxy_reader.set_read_timeout(Some(Duration::from_millis(100)));
    let closed = Arc::new(AtomicBool::new(false));
    let finish_closed = Arc::clone(&closed);
    let finish: Arc<CloseCallback> = Arc::new(move || {
        if !finish_closed.swap(true, Ordering::AcqRel)
            && let Some(close) = &close
        {
            close();
        }
    });

    let receive_closed = Arc::clone(&closed);
    let receive_shutdown = Arc::clone(&engine_shutdown);
    let receive_finish = Arc::clone(&finish);
    thread::spawn(move || {
        let mut packet = vec![0; u16::MAX as usize];
        while !receive_closed.load(Ordering::Acquire) && !receive_shutdown.load(Ordering::Acquire) {
            match receive(&mut packet) {
                Ok(0) => break,
                Ok(length) if length <= packet.len() => {
                    if proxy.send(&packet[..length]).is_err() {
                        break;
                    }
                }
                Ok(_) => break,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => break,
            }
        }
        receive_finish();
    });

    thread::spawn(move || {
        let mut packet = vec![0; u16::MAX as usize];
        while !closed.load(Ordering::Acquire) && !engine_shutdown.load(Ordering::Acquire) {
            match proxy_reader.recv(&mut packet) {
                Ok(0) => break,
                Ok(length) => {
                    if send(&packet[..length]).is_err() {
                        break;
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => break,
            }
        }
        finish();
    });
}

#[cfg(unix)]
fn tcp_stream_into_cronet_fd(stream: TcpStream) -> std::io::Result<i32> {
    use std::os::fd::IntoRawFd;

    Ok(stream.into_raw_fd())
}

#[cfg(unix)]
fn udp_socket_into_cronet_fd(socket: UdpSocket) -> std::io::Result<i32> {
    use std::os::fd::IntoRawFd;

    Ok(socket.into_raw_fd())
}

#[cfg(windows)]
fn tcp_stream_into_cronet_fd(stream: TcpStream) -> std::io::Result<i32> {
    use std::os::windows::io::{AsRawSocket, IntoRawSocket};

    i32::try_from(stream.as_raw_socket())
        .map_err(|_| std::io::Error::other("Windows socket handle does not fit Cronet int"))
        .map(|_| stream.into_raw_socket() as i32)
}

#[cfg(windows)]
fn udp_socket_into_cronet_fd(socket: UdpSocket) -> std::io::Result<i32> {
    use std::os::windows::io::{AsRawSocket, IntoRawSocket};

    i32::try_from(socket.as_raw_socket())
        .map_err(|_| std::io::Error::other("Windows socket handle does not fit Cronet int"))
        .map(|_| socket.into_raw_socket() as i32)
}

/// Result returned by a custom UDP dialer.
#[derive(Clone, Debug)]
pub struct UdpDialResult {
    /// Socket descriptor, or a negative Chromium net error.
    pub fd: i32,
    /// Local IP literal reported to Cronet.
    pub local_address: String,
    /// Local UDP port.
    pub local_port: u16,
}

struct BridgeShutdownGuard(Arc<AtomicBool>);

impl Drop for BridgeShutdownGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

struct NetworkHookState {
    tcp: Option<Box<TcpDialer>>,
    udp: Option<Box<UdpDialer>>,
    _keepalive: Vec<Box<dyn Any + Send + Sync>>,
}

fn is_dns_endpoint(address: &str, port: u16) -> bool {
    port == 53
        && matches!(
            address.trim_matches(['[', ']']),
            "127.0.0.1" | "::1" | "localhost"
        )
}

unsafe extern "C" fn tcp_dialer_trampoline(
    context: *mut core::ffi::c_void,
    address: *const core::ffi::c_char,
    port: u16,
) -> i32 {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if context.is_null() || address.is_null() {
            return -104;
        }
        // SAFETY: SagerNet Cronet supplies the context and a callback-scoped C string.
        let state = unsafe { &*context.cast::<NetworkHookState>() };
        // SAFETY: Checked non-null and native contract guarantees NUL termination.
        let address = unsafe { CStr::from_ptr(address) }.to_string_lossy();
        state
            .tcp
            .as_ref()
            .map_or(-104, |dialer| dialer(&address, port))
    }));
    outcome.unwrap_or_else(|_| std::process::abort())
}

unsafe extern "C" fn udp_dialer_trampoline(
    context: *mut core::ffi::c_void,
    address: *const core::ffi::c_char,
    port: u16,
    out_local_address: *mut core::ffi::c_char,
    out_local_port: *mut u16,
) -> i32 {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if context.is_null() || address.is_null() {
            return -104;
        }
        // SAFETY: SagerNet Cronet supplies live callback arguments.
        let state = unsafe { &*context.cast::<NetworkHookState>() };
        // SAFETY: Address is a callback-scoped NUL-terminated string.
        let address = unsafe { CStr::from_ptr(address) }.to_string_lossy();
        let Some(dialer) = state.udp.as_ref() else {
            return -104;
        };
        let result = dialer(&address, port);
        if !out_local_port.is_null() {
            // SAFETY: Optional output pointer is writable for the callback.
            unsafe { *out_local_port = result.local_port };
        }
        if !out_local_address.is_null()
            && !result.local_address.is_empty()
            && result.local_address.len() <= 45
        {
            let bytes = result.local_address.as_bytes();
            // SAFETY: Native API reserves room for an IPv6 literal plus NUL.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    out_local_address.cast(),
                    bytes.len(),
                );
                *out_local_address.add(bytes.len()) = 0;
            }
        }
        result.fd
    }));
    outcome.unwrap_or_else(|_| std::process::abort())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn split_stream_bridge_copies_both_directions_and_closes_once() {
        let (mut application, transport) = tcp_loopback_pair().unwrap();
        let (mut cronet, proxy) = tcp_loopback_pair().unwrap();
        application
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        cronet
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();

        let transport_reader = transport.try_clone().unwrap();
        let transport_close = transport.try_clone().unwrap();
        let close_count = Arc::new(AtomicUsize::new(0));
        let callback_count = Arc::clone(&close_count);
        bridge_split_stream(
            SplitStream::new(transport_reader, transport).on_close(move || {
                callback_count.fetch_add(1, Ordering::AcqRel);
                let _ = transport_close.shutdown(Shutdown::Both);
            }),
            proxy,
        );

        application.write_all(b"from transport").unwrap();
        let mut inbound = [0; 14];
        cronet.read_exact(&mut inbound).unwrap();
        assert_eq!(&inbound, b"from transport");

        cronet.write_all(b"from cronet").unwrap();
        let mut outbound = [0; 11];
        application.read_exact(&mut outbound).unwrap();
        assert_eq!(&outbound, b"from cronet");

        cronet.shutdown(Shutdown::Both).unwrap();
        for _ in 0..100 {
            if close_count.load(Ordering::Acquire) == 1 {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(close_count.load(Ordering::Acquire), 1);
    }

    #[test]
    fn split_datagram_bridge_preserves_packet_boundaries_and_closes_once() {
        let (application, transport) = udp_loopback_pair().unwrap();
        let (cronet, proxy) = udp_loopback_pair().unwrap();
        application
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        transport
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();
        cronet
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();

        let receive_socket = Arc::new(transport);
        let send_socket = Arc::clone(&receive_socket);
        let close_count = Arc::new(AtomicUsize::new(0));
        let callback_count = Arc::clone(&close_count);
        let close_socket = Arc::clone(&receive_socket);
        let shutdown = Arc::new(AtomicBool::new(false));
        bridge_split_datagram(
            SplitDatagram::new(
                move |packet| receive_socket.recv(packet),
                move |packet| send_socket.send(packet),
                "192.0.2.10",
                12345,
            )
            .on_close(move || {
                callback_count.fetch_add(1, Ordering::AcqRel);
                let _ = close_socket.set_nonblocking(true);
            }),
            proxy,
            Arc::clone(&shutdown),
        );

        application.send(b"first packet").unwrap();
        let mut inbound = [0; 64];
        let inbound_length = cronet.recv(&mut inbound).unwrap();
        assert_eq!(&inbound[..inbound_length], b"first packet");

        cronet.send(b"second packet").unwrap();
        let mut outbound = [0; 64];
        let outbound_length = application.recv(&mut outbound).unwrap();
        assert_eq!(&outbound[..outbound_length], b"second packet");

        drop(cronet);
        shutdown.store(true, Ordering::Release);
        for _ in 0..100 {
            if close_count.load(Ordering::Acquire) == 1 {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(close_count.load(Ordering::Acquire), 1);
    }
}
