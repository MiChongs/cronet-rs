use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    sync::Mutex,
};

use crate::{Engine, Executor, NetworkError, ResponseInfo, sys};

/// Why Cronet considers a request finished.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinishedReason {
    /// The request completed successfully.
    Succeeded,
    /// The request failed.
    Failed,
    /// The request was canceled.
    Canceled,
    /// A value introduced by a newer Cronet SDK.
    Unknown(i32),
}

impl From<i32> for FinishedReason {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Succeeded,
            1 => Self::Failed,
            2 => Self::Canceled,
            other => Self::Unknown(other),
        }
    }
}

/// Owned timing and byte-count snapshot from `Cronet_Metrics`.
#[derive(Clone, Debug)]
pub struct Metrics {
    /// Request start time in milliseconds since the Unix epoch.
    pub request_start: Option<i64>,
    /// DNS lookup start time.
    pub dns_start: Option<i64>,
    /// DNS lookup end time.
    pub dns_end: Option<i64>,
    /// Connection attempt start time.
    pub connect_start: Option<i64>,
    /// Connection attempt end time.
    pub connect_end: Option<i64>,
    /// TLS handshake start time.
    pub ssl_start: Option<i64>,
    /// TLS handshake end time.
    pub ssl_end: Option<i64>,
    /// Request body send start time.
    pub sending_start: Option<i64>,
    /// Request body send end time.
    pub sending_end: Option<i64>,
    /// Server push start time.
    pub push_start: Option<i64>,
    /// Server push end time.
    pub push_end: Option<i64>,
    /// First response byte time.
    pub response_start: Option<i64>,
    /// Request completion time.
    pub request_end: Option<i64>,
    /// Whether Cronet reused an existing socket.
    pub socket_reused: bool,
    /// Number of request bytes sent.
    pub sent_byte_count: i64,
    /// Number of response bytes received.
    pub received_byte_count: i64,
}

impl Metrics {
    pub(crate) unsafe fn copy_from(raw: sys::Cronet_MetricsPtr) -> Option<Self> {
        if raw.is_null() {
            return None;
        }
        // SAFETY: The finished-info callback keeps Metrics and its DateTime
        // children live while this owned snapshot is copied.
        unsafe {
            Some(Self {
                request_start: copy_date_time(sys::Cronet_Metrics_request_start_get(raw)),
                dns_start: copy_date_time(sys::Cronet_Metrics_dns_start_get(raw)),
                dns_end: copy_date_time(sys::Cronet_Metrics_dns_end_get(raw)),
                connect_start: copy_date_time(sys::Cronet_Metrics_connect_start_get(raw)),
                connect_end: copy_date_time(sys::Cronet_Metrics_connect_end_get(raw)),
                ssl_start: copy_date_time(sys::Cronet_Metrics_ssl_start_get(raw)),
                ssl_end: copy_date_time(sys::Cronet_Metrics_ssl_end_get(raw)),
                sending_start: copy_date_time(sys::Cronet_Metrics_sending_start_get(raw)),
                sending_end: copy_date_time(sys::Cronet_Metrics_sending_end_get(raw)),
                push_start: copy_date_time(sys::Cronet_Metrics_push_start_get(raw)),
                push_end: copy_date_time(sys::Cronet_Metrics_push_end_get(raw)),
                response_start: copy_date_time(sys::Cronet_Metrics_response_start_get(raw)),
                request_end: copy_date_time(sys::Cronet_Metrics_request_end_get(raw)),
                socket_reused: sys::Cronet_Metrics_socket_reused_get(raw),
                sent_byte_count: sys::Cronet_Metrics_sent_byte_count_get(raw),
                received_byte_count: sys::Cronet_Metrics_received_byte_count_get(raw),
            })
        }
    }
}

unsafe fn copy_date_time(raw: sys::Cronet_DateTimePtr) -> Option<i64> {
    (!raw.is_null()).then(|| {
        // SAFETY: The caller guarantees the callback-scoped DateTime is live.
        unsafe { sys::Cronet_DateTime_value_get(raw) }
    })
}

/// Owned snapshot supplied to a request-finished listener.
#[derive(Clone, Debug)]
pub struct RequestFinishedInfo {
    /// Final request outcome.
    pub reason: FinishedReason,
    /// Timing metrics, when Cronet collected them.
    pub metrics: Option<Metrics>,
    /// Number of opaque native annotations attached to the request.
    pub annotation_count: usize,
}

impl RequestFinishedInfo {
    pub(crate) unsafe fn copy_from(raw: sys::Cronet_RequestFinishedInfoPtr) -> Option<Self> {
        if raw.is_null() {
            return None;
        }
        // SAFETY: All values are copied during the callback.
        unsafe {
            Some(Self {
                reason: FinishedReason::from(sys::Cronet_RequestFinishedInfo_finished_reason_get(
                    raw,
                ) as i32),
                metrics: Metrics::copy_from(sys::Cronet_RequestFinishedInfo_metrics_get(raw)),
                annotation_count: usize::try_from(
                    sys::Cronet_RequestFinishedInfo_annotations_size(raw),
                )
                .expect("annotation count does not fit usize"),
            })
        }
    }
}

/// One completed Cronet request and its optional response/error snapshots.
#[derive(Clone, Debug)]
pub struct RequestFinished {
    /// Completion metadata. Cronet normally always supplies this value.
    pub info: Option<RequestFinishedInfo>,
    /// Last response information, if a response was received.
    pub response: Option<ResponseInfo>,
    /// Network error for failed requests.
    pub error: Option<NetworkError>,
}

type FinishedHandler = dyn FnMut(RequestFinished) + Send;

struct ListenerState {
    handler: Mutex<Box<FinishedHandler>>,
}

/// Registration of an engine-wide request-finished callback.
///
/// Dropping this value removes the listener from the engine before destroying
/// its native callback object. The supplied executor must outlive the
/// registration.
pub struct RequestFinishedListener<'engine, 'executor> {
    engine: &'engine Engine,
    _executor: &'executor Executor,
    raw: NonNull<sys::Cronet_RequestFinishedInfoListener>,
    state: NonNull<ListenerState>,
}

impl Engine {
    /// Registers an engine-wide request-finished callback.
    pub fn add_request_finished_listener<'engine, 'executor>(
        &'engine self,
        executor: &'executor Executor,
        handler: impl FnMut(RequestFinished) + Send + 'static,
    ) -> RequestFinishedListener<'engine, 'executor> {
        let state = NonNull::from(Box::leak(Box::new(ListenerState {
            handler: Mutex::new(Box::new(handler)),
        })));
        // SAFETY: The trampoline matches the generated callback ABI.
        let raw = unsafe {
            sys::Cronet_RequestFinishedInfoListener_CreateWith(Some(on_request_finished))
        };
        let raw =
            NonNull::new(raw).expect("Cronet_RequestFinishedInfoListener_CreateWith returned null");
        // SAFETY: State is heap-stable and remains live for the registration.
        unsafe {
            sys::Cronet_RequestFinishedInfoListener_SetClientContext(
                raw.as_ptr(),
                state.as_ptr().cast(),
            );
            sys::Cronet_Engine_AddRequestFinishedListener(
                self.as_raw(),
                raw.as_ptr(),
                executor.as_raw(),
            );
        }
        RequestFinishedListener {
            engine: self,
            _executor: executor,
            raw,
            state,
        }
    }
}

impl Drop for RequestFinishedListener<'_, '_> {
    fn drop(&mut self) {
        // SAFETY: The engine, listener and executor outlive this registration.
        unsafe {
            sys::Cronet_Engine_RemoveRequestFinishedListener(
                self.engine.as_raw(),
                self.raw.as_ptr(),
            );
            sys::Cronet_RequestFinishedInfoListener_SetClientContext(
                self.raw.as_ptr(),
                core::ptr::null_mut(),
            );
            sys::Cronet_RequestFinishedInfoListener_Destroy(self.raw.as_ptr());
            drop(Box::from_raw(self.state.as_ptr()));
        }
    }
}

unsafe extern "C" fn on_request_finished(
    listener: sys::Cronet_RequestFinishedInfoListenerPtr,
    request_info: sys::Cronet_RequestFinishedInfoPtr,
    response_info: sys::Cronet_UrlResponseInfoPtr,
    error: sys::Cronet_ErrorPtr,
) {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        if listener.is_null() {
            return;
        }
        // SAFETY: The registration installed this context and removes it before
        // destroying the state.
        let state = unsafe {
            sys::Cronet_RequestFinishedInfoListener_GetClientContext(listener)
                .cast::<ListenerState>()
        };
        if state.is_null() {
            return;
        }
        let event = RequestFinished {
            // SAFETY: Native callback values remain live for this call.
            info: unsafe { RequestFinishedInfo::copy_from(request_info) },
            response: {
                // SAFETY: ResponseInfo copies all callback-scoped fields.
                unsafe { ResponseInfo::copy_from(response_info) }
            },
            error: if error.is_null() {
                None
            } else {
                // SAFETY: NetworkError copies all callback-scoped fields.
                Some(unsafe { NetworkError::copy_from(error) })
            },
        };
        // SAFETY: Context points to the ListenerState owned by the live
        // registration.
        let mut handler = unsafe { &*state }
            .handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        handler(event);
    }));
    if outcome.is_err() {
        std::process::abort();
    }
}
