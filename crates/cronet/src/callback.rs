use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    sync::{Condvar, Mutex},
};

use crate::{NetworkError, ResponseInfo, sys};

/// Borrowed access to the active native request inside a callback.
#[derive(Clone, Copy)]
pub struct RequestHandle(sys::Cronet_UrlRequestPtr);

impl RequestHandle {
    /// Accepts the currently reported redirect.
    pub fn follow_redirect(self) -> crate::Result<()> {
        // SAFETY: Callback guarantees this request is live.
        crate::error::Error::from_result(unsafe { sys::Cronet_UrlRequest_FollowRedirect(self.0) })
    }

    /// Cancels the request.
    pub fn cancel(self) {
        // SAFETY: Callback guarantees this request is live.
        unsafe { sys::Cronet_UrlRequest_Cancel(self.0) };
    }

    /// Requests another body read into a native buffer.
    ///
    /// # Safety
    ///
    /// The buffer must remain valid and unused until `on_read_completed`.
    pub unsafe fn read(self, buffer: &mut crate::Buffer) -> crate::Result<()> {
        // SAFETY: Caller upholds the asynchronous buffer lifetime contract.
        let result = crate::error::Error::from_result(unsafe {
            sys::Cronet_UrlRequest_Read(self.0, buffer.as_raw())
        });
        if result.is_ok() {
            buffer.transfer_to_native();
        }
        result
    }

    /// Exposes the borrowed native request.
    pub fn as_raw(self) -> sys::Cronet_UrlRequestPtr {
        self.0
    }
}

/// Callback interface implemented by Rust applications.
pub trait UrlRequestHandler: Send + 'static {
    /// Called for an HTTP redirect.
    fn on_redirect(
        &mut self,
        request: RequestHandle,
        info: Option<ResponseInfo>,
        new_location: String,
    ) {
        let _ = (request, info, new_location);
    }

    /// Called when response headers are available.
    fn on_response_started(&mut self, request: RequestHandle, info: ResponseInfo) {
        let _ = (request, info);
    }

    /// Called after a supplied read buffer has been filled.
    fn on_read_completed(
        &mut self,
        request: RequestHandle,
        info: ResponseInfo,
        buffer: sys::Cronet_BufferPtr,
        bytes_read: u64,
    ) {
        let _ = (request, info, buffer, bytes_read);
    }

    /// Called after successful completion.
    fn on_succeeded(&mut self, request: RequestHandle, info: ResponseInfo) {
        let _ = (request, info);
    }

    /// Called after network or callback failure.
    fn on_failed(
        &mut self,
        request: RequestHandle,
        info: Option<ResponseInfo>,
        error: NetworkError,
    ) {
        let _ = (request, info, error);
    }

    /// Called after cancellation completes.
    fn on_canceled(&mut self, request: RequestHandle, info: Option<ResponseInfo>) {
        let _ = (request, info);
    }
}

struct CallbackState {
    handler: Mutex<Box<dyn UrlRequestHandler>>,
    active: Mutex<usize>,
    idle: Condvar,
    terminal: Mutex<bool>,
    terminal_event: Condvar,
}

/// Native Cronet callback backed by a Rust [`UrlRequestHandler`].
pub struct UrlRequestCallback {
    raw: NonNull<sys::Cronet_UrlRequestCallback>,
    state: NonNull<CallbackState>,
}

impl UrlRequestCallback {
    /// Creates a callback adapter.
    pub fn new(handler: impl UrlRequestHandler) -> Self {
        let state = Box::new(CallbackState {
            handler: Mutex::new(Box::new(handler)),
            active: Mutex::new(0),
            idle: Condvar::new(),
            terminal: Mutex::new(false),
            terminal_event: Condvar::new(),
        });
        let state = NonNull::from(Box::leak(state));
        // SAFETY: All trampolines have their generated C signatures.
        let raw = unsafe {
            sys::Cronet_UrlRequestCallback_CreateWith(
                Some(on_redirect),
                Some(on_response_started),
                Some(on_read_completed),
                Some(on_succeeded),
                Some(on_failed),
                Some(on_canceled),
            )
        };
        let raw = NonNull::new(raw).expect("Cronet_UrlRequestCallback_CreateWith returned null");
        // SAFETY: Context is owned by this callback and read only by trampolines.
        unsafe {
            sys::Cronet_UrlRequestCallback_SetClientContext(raw.as_ptr(), state.as_ptr().cast());
        }
        Self { raw, state }
    }

    /// Returns the native callback pointer.
    pub fn as_raw(&self) -> sys::Cronet_UrlRequestCallbackPtr {
        self.raw.as_ptr()
    }

    pub(crate) fn wait_idle(&self) {
        // SAFETY: State is owned by this adapter.
        let state = unsafe { self.state.as_ref() };
        let active = state
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        drop(
            state
                .idle
                .wait_while(active, |count| *count != 0)
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    }

    pub(crate) fn prepare_request(&mut self) {
        // SAFETY: Exclusive adapter borrow prevents a concurrent request.
        let state = unsafe { self.state.as_ref() };
        *state
            .terminal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
    }

    pub(crate) fn wait_terminal(&self) {
        // SAFETY: State is owned by this adapter.
        let state = unsafe { self.state.as_ref() };
        let terminal = state
            .terminal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        drop(
            state
                .terminal_event
                .wait_while(terminal, |terminal| !*terminal)
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        self.wait_idle();
    }
}

impl Drop for UrlRequestCallback {
    fn drop(&mut self) {
        self.wait_idle();
        // SAFETY: Safe requests borrow this callback.
        unsafe {
            sys::Cronet_UrlRequestCallback_SetClientContext(
                self.raw.as_ptr(),
                core::ptr::null_mut(),
            );
            sys::Cronet_UrlRequestCallback_Destroy(self.raw.as_ptr());
            drop(Box::from_raw(self.state.as_ptr()));
        }
    }
}

unsafe fn with_handler(
    callback: sys::Cronet_UrlRequestCallbackPtr,
    call: impl FnOnce(&mut dyn UrlRequestHandler),
) {
    // SAFETY: Context belongs to UrlRequestCallback and lives during callbacks.
    let context = unsafe { sys::Cronet_UrlRequestCallback_GetClientContext(callback) }
        .cast::<CallbackState>();
    if context.is_null() {
        return;
    }
    // SAFETY: Pointer and synchronization state remain live for the callback.
    let state = unsafe { &*context };
    {
        let mut active = state
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *active += 1;
    }
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: Pointer and mutex are live. Poisoning is recovered because a
        // panic never crosses FFI and subsequent cancellation should proceed.
        let mut handler = state
            .handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        call(handler.as_mut());
    }));
    {
        let mut active = state
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *active -= 1;
        if *active == 0 {
            state.idle.notify_all();
        }
    }
    if outcome.is_err() {
        std::process::abort();
    }
}

unsafe fn mark_terminal(callback: sys::Cronet_UrlRequestCallbackPtr) {
    // SAFETY: Context belongs to UrlRequestCallback and lives during callbacks.
    let context = unsafe { sys::Cronet_UrlRequestCallback_GetClientContext(callback) }
        .cast::<CallbackState>();
    // SAFETY: Non-null context was installed by UrlRequestCallback::new.
    if let Some(state) = unsafe { context.as_ref() } {
        *state
            .terminal
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        state.terminal_event.notify_all();
    }
}

unsafe extern "C" fn on_redirect(
    callback: sys::Cronet_UrlRequestCallbackPtr,
    request: sys::Cronet_UrlRequestPtr,
    info: sys::Cronet_UrlResponseInfoPtr,
    location: sys::Cronet_String,
) {
    // SAFETY: Cronet owns callback arguments for this invocation.
    // SAFETY: Cronet owns callback arguments for this invocation.
    let info = unsafe { ResponseInfo::copy_from(info) };
    // SAFETY: Location is a Cronet string valid for this callback.
    let location = unsafe { crate::model::copy_string(location) };
    // SAFETY: Callback context remains live during this invocation.
    unsafe {
        with_handler(callback, |handler| {
            handler.on_redirect(RequestHandle(request), info, location);
        })
    };
}

unsafe extern "C" fn on_response_started(
    callback: sys::Cronet_UrlRequestCallbackPtr,
    request: sys::Cronet_UrlRequestPtr,
    info: sys::Cronet_UrlResponseInfoPtr,
) {
    // SAFETY: Cronet owns response info for this invocation.
    let Some(info) = (unsafe { ResponseInfo::copy_from(info) }) else {
        return;
    };
    // SAFETY: Callback context remains live during this invocation.
    unsafe {
        with_handler(callback, |handler| {
            handler.on_response_started(RequestHandle(request), info);
        })
    };
}

unsafe extern "C" fn on_read_completed(
    callback: sys::Cronet_UrlRequestCallbackPtr,
    request: sys::Cronet_UrlRequestPtr,
    info: sys::Cronet_UrlResponseInfoPtr,
    buffer: sys::Cronet_BufferPtr,
    bytes_read: u64,
) {
    // SAFETY: Cronet owns response info for this invocation.
    let Some(info) = (unsafe { ResponseInfo::copy_from(info) }) else {
        return;
    };
    // SAFETY: Callback context remains live during this invocation.
    unsafe {
        with_handler(callback, |handler| {
            handler.on_read_completed(RequestHandle(request), info, buffer, bytes_read);
        })
    };
}

unsafe extern "C" fn on_succeeded(
    callback: sys::Cronet_UrlRequestCallbackPtr,
    request: sys::Cronet_UrlRequestPtr,
    info: sys::Cronet_UrlResponseInfoPtr,
) {
    // SAFETY: Cronet owns response info for this invocation.
    let Some(info) = (unsafe { ResponseInfo::copy_from(info) }) else {
        return;
    };
    // SAFETY: Callback context remains live during this invocation.
    unsafe {
        with_handler(callback, |handler| {
            handler.on_succeeded(RequestHandle(request), info);
        })
    };
    // SAFETY: Callback context remains live until trampoline returns.
    unsafe { mark_terminal(callback) };
}

unsafe extern "C" fn on_failed(
    callback: sys::Cronet_UrlRequestCallbackPtr,
    request: sys::Cronet_UrlRequestPtr,
    info: sys::Cronet_UrlResponseInfoPtr,
    error: sys::Cronet_ErrorPtr,
) {
    if error.is_null() {
        return;
    }
    // SAFETY: Cronet owns callback arguments for this invocation.
    let info = unsafe { ResponseInfo::copy_from(info) };
    // SAFETY: Non-null error is valid for this callback.
    let error = unsafe { NetworkError::copy_from(error) };
    // SAFETY: Callback context remains live during this invocation.
    unsafe {
        with_handler(callback, |handler| {
            handler.on_failed(RequestHandle(request), info, error);
        })
    };
    // SAFETY: Callback context remains live until trampoline returns.
    unsafe { mark_terminal(callback) };
}

unsafe extern "C" fn on_canceled(
    callback: sys::Cronet_UrlRequestCallbackPtr,
    request: sys::Cronet_UrlRequestPtr,
    info: sys::Cronet_UrlResponseInfoPtr,
) {
    // SAFETY: Cronet owns response info for this invocation.
    let info = unsafe { ResponseInfo::copy_from(info) };
    // SAFETY: Callback context remains live during this invocation.
    unsafe {
        with_handler(callback, |handler| {
            handler.on_canceled(RequestHandle(request), info);
        })
    };
    // SAFETY: Callback context remains live until trampoline returns.
    unsafe { mark_terminal(callback) };
}
