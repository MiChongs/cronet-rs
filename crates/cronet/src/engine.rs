use std::{
    any::Any,
    ffi::{CStr, CString},
    net::{TcpStream, UdpSocket},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    sync::{Arc, RwLock, atomic::AtomicBool},
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
            previous_tcp
                .as_ref()
                .map_or(NetError::CONNECTION_FAILED.code(), |dialer| {
                    dialer(address, port)
                })
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
                || UdpDialResult {
                    fd: NetError::CONNECTION_FAILED.code(),
                    local_address: String::new(),
                    local_port: 0,
                },
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
