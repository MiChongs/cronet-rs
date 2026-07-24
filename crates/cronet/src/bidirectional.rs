use std::{
    collections::HashMap,
    ffi::{CStr, CString},
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{Engine, Header, sys};

/// Borrowed handle passed to bidirectional-stream callbacks.
#[derive(Clone, Copy)]
pub struct BidirectionalStreamHandle(*mut sys::bidirectional_stream);

impl BidirectionalStreamHandle {
    /// Starts one asynchronous response read.
    ///
    /// # Safety
    ///
    /// `buffer` must remain stable and unused until `on_read_completed`.
    pub unsafe fn read(self, buffer: &mut [u8]) -> i32 {
        let len = i32::try_from(buffer.len()).unwrap_or(i32::MAX);
        // SAFETY: Caller upholds buffer lifetime and native stream is callback-live.
        unsafe { sys::bidirectional_stream_read(self.0, buffer.as_mut_ptr().cast(), len) }
    }

    /// Starts one asynchronous request-body write.
    ///
    /// # Safety
    ///
    /// `buffer` must remain stable and unused until `on_write_completed`.
    pub unsafe fn write(self, buffer: &[u8], end_of_stream: bool) -> i32 {
        let len = i32::try_from(buffer.len()).unwrap_or(i32::MAX);
        // SAFETY: Caller upholds buffer lifetime and native stream is callback-live.
        unsafe {
            sys::bidirectional_stream_write(self.0, buffer.as_ptr().cast(), len, end_of_stream)
        }
    }

    /// Flushes writes when auto-flush is disabled.
    pub fn flush(self) {
        // SAFETY: Native stream is live during its callback.
        unsafe { sys::bidirectional_stream_flush(self.0) };
    }

    /// Cancels stream processing.
    pub fn cancel(self) {
        // SAFETY: Native stream is live during its callback.
        unsafe { sys::bidirectional_stream_cancel(self.0) };
    }

    /// Returns the native stream pointer.
    pub fn as_raw(self) -> *mut sys::bidirectional_stream {
        self.0
    }
}

/// Events produced by the SagerNet Cronet bidirectional-stream API.
///
/// These methods run synchronously on Cronet's network thread and must not
/// block, perform disk I/O, or call engine shutdown.
pub trait BidirectionalStreamHandler: Send + 'static {
    /// Stream can begin reading and writing.
    fn on_stream_ready(&mut self, stream: BidirectionalStreamHandle);

    /// Initial response headers arrived.
    fn on_response_headers(
        &mut self,
        stream: BidirectionalStreamHandle,
        headers: Vec<Header>,
        negotiated_protocol: String,
    );

    /// One response read completed.
    fn on_read_completed(&mut self, stream: BidirectionalStreamHandle, bytes_read: i32);

    /// One request write completed.
    fn on_write_completed(&mut self, stream: BidirectionalStreamHandle);

    /// Response trailers arrived.
    fn on_response_trailers(&mut self, stream: BidirectionalStreamHandle, trailers: Vec<Header>) {
        let _ = (stream, trailers);
    }

    /// Both directions completed successfully.
    fn on_succeeded(&mut self, stream: BidirectionalStreamHandle);

    /// Stream failed with a Chromium net error.
    fn on_failed(&mut self, stream: BidirectionalStreamHandle, net_error: i32);

    /// Cancellation completed.
    fn on_canceled(&mut self, stream: BidirectionalStreamHandle);
}

struct CallbackEntry {
    handler: Mutex<Box<dyn BidirectionalStreamHandler>>,
    destroyed: AtomicBool,
}

type Registry = Mutex<HashMap<usize, Arc<CallbackEntry>>>;

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Owned SagerNet Cronet bidirectional stream.
pub struct BidirectionalStream<'engine> {
    raw: NonNull<sys::bidirectional_stream>,
    entry: Arc<CallbackEntry>,
    _engine: PhantomData<&'engine Engine>,
}

// SAFETY: SagerNet's stream API permits operations and asynchronous destroy
// from arbitrary application threads. Rust enforces at most one mutable buffer
// per submitted read/write at the safe connection layer.
unsafe impl Send for BidirectionalStream<'_> {}
// SAFETY: Native stream synchronizes independent read and write directions;
// methods expose only shared ownership and buffer lifetime remains explicit.
unsafe impl Sync for BidirectionalStream<'_> {}

impl Engine {
    /// Creates a bidirectional H2/H3 stream using SagerNet Cronet.
    pub fn create_bidirectional_stream(
        &self,
        handler: impl BidirectionalStreamHandler,
    ) -> BidirectionalStream<'_> {
        // SAFETY: Engine is live.
        let stream_engine = unsafe { sys::Cronet_Engine_GetStreamEngine(self.as_raw()) };
        assert!(
            !stream_engine.is_null(),
            "Cronet stream engine is unavailable"
        );
        let entry = Arc::new(CallbackEntry {
            handler: Mutex::new(Box::new(handler)),
            destroyed: AtomicBool::new(false),
        });
        // SAFETY: Engine and static callback table are live.
        let raw = unsafe {
            sys::bidirectional_stream_create(stream_engine, core::ptr::null_mut(), &CALLBACKS)
        };
        let raw = NonNull::new(raw).expect("bidirectional_stream_create returned null");
        registry()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(raw.as_ptr() as usize, Arc::clone(&entry));
        BidirectionalStream {
            raw,
            entry,
            _engine: PhantomData,
        }
    }
}

impl BidirectionalStream<'_> {
    /// Controls automatic flushing after every write.
    pub fn disable_auto_flush(&self, disabled: bool) {
        // SAFETY: Owned stream is live.
        unsafe { sys::bidirectional_stream_disable_auto_flush(self.raw.as_ptr(), disabled) };
    }

    /// Delays QUIC request headers until the first flush.
    pub fn delay_request_headers_until_flush(&self, delayed: bool) {
        // SAFETY: Owned stream is live.
        unsafe {
            sys::bidirectional_stream_delay_request_headers_until_flush(self.raw.as_ptr(), delayed)
        };
    }

    /// Starts a CONNECT or ordinary bidirectional request.
    pub fn start(
        &self,
        method: &str,
        url: &str,
        headers: &[Header],
        priority: i32,
        end_of_stream: bool,
    ) -> bool {
        let method = CString::new(method).expect("method cannot contain NUL bytes");
        let url = CString::new(url).expect("URL cannot contain NUL bytes");
        let owned: Vec<_> = headers
            .iter()
            .map(|header| {
                (
                    CString::new(header.name.as_str())
                        .expect("header name cannot contain NUL bytes"),
                    CString::new(header.value.as_str())
                        .expect("header value cannot contain NUL bytes"),
                )
            })
            .collect();
        let mut raw_headers: Vec<_> = owned
            .iter()
            .map(|(key, value)| sys::bidirectional_stream_header {
                key: key.as_ptr(),
                value: value.as_ptr(),
            })
            .collect();
        let array = sys::bidirectional_stream_header_array {
            count: raw_headers.len(),
            capacity: raw_headers.len(),
            headers: raw_headers.as_mut_ptr(),
        };
        // SAFETY: All strings and header storage live through the call; native
        // start copies request metadata.
        unsafe {
            sys::bidirectional_stream_start(
                self.raw.as_ptr(),
                url.as_ptr(),
                priority,
                method.as_ptr(),
                &array,
                end_of_stream,
            ) == 0
        }
    }

    /// Starts an asynchronous read.
    ///
    /// # Safety
    ///
    /// Buffer storage must remain stable until the read callback.
    pub unsafe fn read(&self, buffer: &mut [u8]) -> i32 {
        // SAFETY: Caller upholds the asynchronous buffer contract.
        unsafe { BidirectionalStreamHandle(self.raw.as_ptr()).read(buffer) }
    }

    /// Starts an asynchronous write.
    ///
    /// # Safety
    ///
    /// Buffer storage must remain stable until the write callback.
    pub unsafe fn write(&self, buffer: &[u8], end_of_stream: bool) -> i32 {
        // SAFETY: Caller upholds the asynchronous buffer contract.
        unsafe { BidirectionalStreamHandle(self.raw.as_ptr()).write(buffer, end_of_stream) }
    }

    /// Flushes pending writes.
    pub fn flush(&self) {
        BidirectionalStreamHandle(self.raw.as_ptr()).flush();
    }

    /// Cancels this stream.
    pub fn cancel(&self) {
        BidirectionalStreamHandle(self.raw.as_ptr()).cancel();
    }

    /// Returns the native stream pointer.
    pub fn as_raw(&self) -> *mut sys::bidirectional_stream {
        self.raw.as_ptr()
    }
}

impl Drop for BidirectionalStream<'_> {
    fn drop(&mut self) {
        self.entry.destroyed.store(true, Ordering::Release);
        registry()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&(self.raw.as_ptr() as usize));
        // SAFETY: Destroy is asynchronous and accepts calls from any thread.
        unsafe {
            let _ = sys::bidirectional_stream_destroy(self.raw.as_ptr());
        }
    }
}

static CALLBACKS: sys::bidirectional_stream_callback = sys::bidirectional_stream_callback {
    on_stream_ready: Some(on_stream_ready),
    on_response_headers_received: Some(on_response_headers),
    on_read_completed: Some(on_read_completed),
    on_write_completed: Some(on_write_completed),
    on_response_trailers_received: Some(on_response_trailers),
    on_succeded: Some(on_succeeded),
    on_failed: Some(on_failed),
    on_canceled: Some(on_canceled),
};

fn with_handler(
    stream: *mut sys::bidirectional_stream,
    call: impl FnOnce(&mut dyn BidirectionalStreamHandler),
) {
    let entry = registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&(stream as usize))
        .cloned();
    let Some(entry) = entry else {
        return;
    };
    if entry.destroyed.load(Ordering::Acquire) {
        return;
    }
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let mut handler = entry
            .handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        call(handler.as_mut());
    }));
    if outcome.is_err() {
        std::process::abort();
    }
}

unsafe fn copy_headers(raw: *mut sys::bidirectional_stream_header_array) -> Vec<Header> {
    if raw.is_null() {
        return Vec::new();
    }
    // SAFETY: Header array is callback-scoped and contains `count` elements.
    let array = unsafe { &*raw };
    if array.headers.is_null() {
        return Vec::new();
    }
    // SAFETY: Native callback guarantees array storage.
    unsafe { core::slice::from_raw_parts(array.headers, array.count) }
        .iter()
        .filter_map(|header| {
            if header.key.is_null() {
                return None;
            }
            // SAFETY: Header strings are valid during callback.
            let name = unsafe { CStr::from_ptr(header.key) }
                .to_string_lossy()
                .into_owned();
            let value = if header.value.is_null() {
                String::new()
            } else {
                // SAFETY: Header value is callback-scoped C string.
                unsafe { CStr::from_ptr(header.value) }
                    .to_string_lossy()
                    .into_owned()
            };
            Some(Header { name, value })
        })
        .collect()
}

unsafe extern "C" fn on_stream_ready(stream: *mut sys::bidirectional_stream) {
    with_handler(stream, |handler| {
        handler.on_stream_ready(BidirectionalStreamHandle(stream));
    });
}

unsafe extern "C" fn on_response_headers(
    stream: *mut sys::bidirectional_stream,
    headers: *mut sys::bidirectional_stream_header_array,
    protocol: *mut core::ffi::c_char,
) {
    // SAFETY: Header array and protocol are callback-scoped.
    let headers = unsafe { copy_headers(headers) };
    let protocol = if protocol.is_null() {
        String::new()
    } else {
        // SAFETY: Checked non-null callback-scoped C string.
        unsafe { CStr::from_ptr(protocol) }
            .to_string_lossy()
            .into_owned()
    };
    with_handler(stream, |handler| {
        handler.on_response_headers(BidirectionalStreamHandle(stream), headers, protocol);
    });
}

unsafe extern "C" fn on_read_completed(
    stream: *mut sys::bidirectional_stream,
    _data: *mut core::ffi::c_char,
    bytes_read: i32,
) {
    with_handler(stream, |handler| {
        handler.on_read_completed(BidirectionalStreamHandle(stream), bytes_read);
    });
}

unsafe extern "C" fn on_write_completed(
    stream: *mut sys::bidirectional_stream,
    _data: *mut core::ffi::c_char,
) {
    with_handler(stream, |handler| {
        handler.on_write_completed(BidirectionalStreamHandle(stream));
    });
}

unsafe extern "C" fn on_response_trailers(
    stream: *mut sys::bidirectional_stream,
    trailers: *mut sys::bidirectional_stream_header_array,
) {
    // SAFETY: Trailer array is callback-scoped.
    let trailers = unsafe { copy_headers(trailers) };
    with_handler(stream, |handler| {
        handler.on_response_trailers(BidirectionalStreamHandle(stream), trailers);
    });
}

unsafe extern "C" fn on_succeeded(stream: *mut sys::bidirectional_stream) {
    with_handler(stream, |handler| {
        handler.on_succeeded(BidirectionalStreamHandle(stream));
    });
    cleanup(stream);
}

unsafe extern "C" fn on_failed(stream: *mut sys::bidirectional_stream, net_error: i32) {
    with_handler(stream, |handler| {
        handler.on_failed(BidirectionalStreamHandle(stream), net_error);
    });
    cleanup(stream);
}

unsafe extern "C" fn on_canceled(stream: *mut sys::bidirectional_stream) {
    with_handler(stream, |handler| {
        handler.on_canceled(BidirectionalStreamHandle(stream));
    });
    cleanup(stream);
}

fn cleanup(stream: *mut sys::bidirectional_stream) {
    registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&(stream as usize));
}
