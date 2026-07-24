use std::{cell::Cell, ffi::CString, marker::PhantomData, ptr::NonNull};

use crate::{
    Buffer, Engine, Executor, RequestParams, Result, UrlRequestCallback, error::Error, sys,
};

/// An allocated request that has not yet been initialized.
pub struct UninitializedRequest(NonNull<sys::Cronet_UrlRequest>);

impl UninitializedRequest {
    /// Allocates a native URL request.
    pub fn new() -> Self {
        // SAFETY: Native constructor takes no arguments.
        let ptr = unsafe { sys::Cronet_UrlRequest_Create() };
        Self(NonNull::new(ptr).expect("Cronet_UrlRequest_Create returned null"))
    }

    /// Connects a request to its engine and application callback objects.
    ///
    /// # Safety
    ///
    /// `callback` and `executor` must be valid Cronet objects and must outlive
    /// the returned request. Their implementations must obey Cronet's threading,
    /// reentrancy and destruction contracts.
    pub unsafe fn initialize<'engine, 'callbacks>(
        self,
        engine: &'engine Engine,
        url: &str,
        params: &RequestParams,
        callback: NonNull<sys::Cronet_UrlRequestCallback>,
        executor: NonNull<sys::Cronet_Executor>,
    ) -> Result<Request<'engine, 'callbacks>> {
        let url = CString::new(url).expect("URL cannot contain NUL bytes");
        // SAFETY: Guaranteed by caller and references passed to this method.
        let result = unsafe {
            sys::Cronet_UrlRequest_InitWithParams(
                self.0.as_ptr(),
                engine.as_raw(),
                url.as_ptr(),
                params.as_ptr(),
                callback.as_ptr(),
                executor.as_ptr(),
            )
        };
        Error::from_result(result)?;
        let ptr = self.0;
        core::mem::forget(self);
        Ok(Request {
            ptr,
            _engine: PhantomData,
            _callbacks: PhantomData,
            callback: None,
            executor: None,
            started: Cell::new(false),
        })
    }

    /// Initializes a request with Rust-owned callback and executor adapters.
    pub fn initialize_with<'resources>(
        self,
        engine: &'resources Engine,
        url: &str,
        params: &RequestParams,
        callback: &'resources mut UrlRequestCallback,
        executor: &'resources Executor,
    ) -> Result<Request<'resources, 'resources>> {
        callback.prepare_request();
        // SAFETY: Returned request borrows the engine, callback and executor,
        // preventing their destruction for the entire native request lifetime.
        unsafe {
            let mut request = self.initialize(
                engine,
                url,
                params,
                NonNull::new(callback.as_raw()).expect("callback pointer is null"),
                NonNull::new(executor.as_raw()).expect("executor pointer is null"),
            )?;
            request.callback = Some(callback);
            request.executor = Some(executor);
            Ok(request)
        }
    }
}

impl Default for UninitializedRequest {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for UninitializedRequest {
    fn drop(&mut self) {
        // SAFETY: This wrapper uniquely owns the request.
        unsafe { sys::Cronet_UrlRequest_Destroy(self.0.as_ptr()) };
    }
}

/// An initialized URL request.
pub struct Request<'engine, 'callbacks> {
    ptr: NonNull<sys::Cronet_UrlRequest>,
    _engine: PhantomData<&'engine Engine>,
    _callbacks: PhantomData<&'callbacks mut ()>,
    callback: Option<&'callbacks UrlRequestCallback>,
    executor: Option<&'callbacks Executor>,
    started: Cell<bool>,
}

impl Request<'_, '_> {
    /// Starts request processing.
    pub fn start(&self) -> Result<()> {
        // SAFETY: Request is initialized and live.
        let result = Error::from_result(unsafe { sys::Cronet_UrlRequest_Start(self.ptr.as_ptr()) });
        if result.is_ok() {
            self.started.set(true);
        }
        result
    }

    /// Accepts the redirect reported by the request callback.
    pub fn follow_redirect(&self) -> Result<()> {
        // SAFETY: Request is initialized and live.
        Error::from_result(unsafe { sys::Cronet_UrlRequest_FollowRedirect(self.ptr.as_ptr()) })
    }

    /// Supplies a buffer for the next asynchronous body read.
    ///
    /// # Safety
    ///
    /// The buffer must remain alive and must not be read, mutated, moved or
    /// destroyed until `OnReadCompleted` returns it to the application.
    pub unsafe fn read(&self, buffer: &mut Buffer) -> Result<()> {
        // SAFETY: Both objects are live for the call. Cronet's callback signals
        // when the application may reuse the buffer; caller upholds that wait.
        let result = Error::from_result(unsafe {
            sys::Cronet_UrlRequest_Read(self.ptr.as_ptr(), buffer.as_raw())
        });
        if result.is_ok() {
            buffer.transfer_to_native();
        }
        result
    }

    /// Cancels request processing. Completion is reported asynchronously.
    pub fn cancel(&self) {
        // SAFETY: Request is initialized and live.
        unsafe { sys::Cronet_UrlRequest_Cancel(self.ptr.as_ptr()) };
    }

    /// Returns whether the request reached a terminal state.
    pub fn is_done(&self) -> bool {
        // SAFETY: Request is initialized and live.
        unsafe { sys::Cronet_UrlRequest_IsDone(self.ptr.as_ptr()) }
    }

    /// Requests an asynchronous snapshot of the native request status.
    pub fn status(&self) -> std::sync::mpsc::Receiver<crate::RequestStatus> {
        crate::status::request_status(self.ptr.as_ptr())
    }

    /// Returns the native pointer for generated-ABI operations.
    pub fn as_raw(&self) -> sys::Cronet_UrlRequestPtr {
        self.ptr.as_ptr()
    }
}

impl Drop for Request<'_, '_> {
    fn drop(&mut self) {
        // SAFETY: This wrapper uniquely owns the native request. Cancel is
        // idempotent for unfinished requests according to Cronet.
        unsafe {
            if self.started.get() && !sys::Cronet_UrlRequest_IsDone(self.ptr.as_ptr()) {
                sys::Cronet_UrlRequest_Cancel(self.ptr.as_ptr());
            }
        }
        if self.started.get()
            && let Some(callback) = self.callback
        {
            callback.wait_terminal();
        }
        if let Some(executor) = self.executor {
            executor.wait_idle();
        }
        // SAFETY: Callback is idle and this wrapper uniquely owns the request.
        unsafe {
            sys::Cronet_UrlRequest_Destroy(self.ptr.as_ptr());
        }
    }
}
