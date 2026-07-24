use std::{
    fmt,
    sync::mpsc::{self, SyncSender},
};

use crate::{
    Buffer, Engine, Executor, Header, NetworkError, RequestParams, ResponseInfo,
    UninitializedRequest, UploadDataProvider, UrlRequestCallback, UrlRequestHandler,
};

/// Configuration for one high-level HTTP request.
#[derive(Clone, Debug)]
pub struct RequestOptions {
    /// Absolute request URL.
    pub url: String,
    /// HTTP method.
    pub method: String,
    /// Request headers.
    pub headers: Vec<Header>,
    /// Whether this request bypasses Cronet's HTTP cache.
    pub disable_cache: bool,
    /// Size of each native response-body read.
    pub read_buffer_size: usize,
    /// Optional in-memory request body. The body is rewindable for redirects.
    pub body: Option<Vec<u8>>,
}

impl RequestOptions {
    /// Creates a GET request.
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            method: "GET".into(),
            headers: Vec::new(),
            disable_cache: false,
            read_buffer_size: 32 * 1024,
            body: None,
        }
    }

    /// Adds a request header.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push(Header {
            name: name.into(),
            value: value.into(),
        });
        self
    }

    /// Sets an in-memory request body.
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(body.into());
        self
    }
}

/// Fully collected HTTP response.
#[derive(Clone, Debug)]
pub struct Response {
    /// Final response metadata.
    pub info: ResponseInfo,
    /// Response body bytes.
    pub body: Vec<u8>,
}

/// Failure from the high-level request API.
#[derive(Debug)]
pub enum RequestError {
    /// Native setup or request state failure.
    Api(crate::Error),
    /// Network failure reported asynchronously.
    Network(NetworkError),
    /// Request was canceled.
    Canceled,
    /// Callback execution ended without a terminal event.
    CallbackDisconnected,
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Api(error) => error.fmt(f),
            Self::Network(error) => write!(f, "Cronet network error: {}", error.message),
            Self::Canceled => f.write_str("Cronet request was canceled"),
            Self::CallbackDisconnected => f.write_str("Cronet callback channel disconnected"),
        }
    }
}

impl std::error::Error for RequestError {}

impl From<crate::Error> for RequestError {
    fn from(value: crate::Error) -> Self {
        Self::Api(value)
    }
}

/// High-level client that runs callbacks on one dedicated Rust thread.
pub struct Client {
    engine: Engine,
    executor: Executor,
}

impl Client {
    /// Creates a client around a started engine.
    pub fn new(engine: Engine) -> std::io::Result<Self> {
        Ok(Self {
            engine,
            executor: Executor::dedicated_thread("cronet-rs-callback")?,
        })
    }

    /// Executes a request and collects its complete response body.
    ///
    /// This blocking API is useful for synchronous applications and also forms
    /// the deterministic core used by async adapters.
    pub fn execute(&self, options: RequestOptions) -> Result<Response, RequestError> {
        let mut params = RequestParams::new();
        params
            .method(&options.method)
            .disable_cache(options.disable_cache);
        for header in options.headers {
            params.header(header);
        }
        let upload = options.body.map(UploadDataProvider::from_bytes);
        if let Some(upload) = upload.as_ref() {
            params.upload(upload, &self.executor);
        }

        let (sender, receiver) = mpsc::sync_channel(1);
        let mut callback = UrlRequestCallback::new(CollectingHandler {
            sender: Some(sender),
            info: None,
            body: Vec::new(),
            buffer: None,
            read_buffer_size: options.read_buffer_size.max(1),
        });
        let request = UninitializedRequest::new().initialize_with(
            &self.engine,
            &options.url,
            &params,
            &mut callback,
            &self.executor,
        )?;
        request.start()?;

        let outcome = receiver
            .recv()
            .unwrap_or(Err(RequestError::CallbackDisconnected));
        drop(request);
        outcome
    }

    /// Borrows the underlying engine for advanced APIs.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }
}

struct CollectingHandler {
    sender: Option<SyncSender<Result<Response, RequestError>>>,
    info: Option<ResponseInfo>,
    body: Vec<u8>,
    buffer: Option<Buffer>,
    read_buffer_size: usize,
}

impl CollectingHandler {
    fn finish(&mut self, outcome: Result<Response, RequestError>) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(outcome);
        }
    }

    fn start_read(&mut self, request: crate::RequestHandle) {
        let buffer = self
            .buffer
            .get_or_insert_with(|| Buffer::new(self.read_buffer_size));
        // SAFETY: Handler owns the buffer until a terminal callback. It neither
        // reads nor mutates it between Read and OnReadCompleted.
        if let Err(error) = unsafe { request.read(buffer) } {
            self.finish(Err(RequestError::Api(error)));
            request.cancel();
        }
    }
}

impl UrlRequestHandler for CollectingHandler {
    fn on_redirect(
        &mut self,
        request: crate::RequestHandle,
        _info: Option<ResponseInfo>,
        _new_location: String,
    ) {
        if let Err(error) = request.follow_redirect() {
            self.finish(Err(RequestError::Api(error)));
            request.cancel();
        }
    }

    fn on_response_started(&mut self, request: crate::RequestHandle, info: ResponseInfo) {
        self.info = Some(info);
        self.start_read(request);
    }

    fn on_read_completed(
        &mut self,
        request: crate::RequestHandle,
        info: ResponseInfo,
        raw_buffer: crate::sys::Cronet_BufferPtr,
        bytes_read: u64,
    ) {
        self.info = Some(info);
        let count = usize::try_from(bytes_read).unwrap_or(usize::MAX);
        if let Some(buffer) = self.buffer.as_ref() {
            assert!(
                buffer.reclaim_from_read(raw_buffer),
                "Cronet returned a different read buffer"
            );
            let count = count.min(buffer.len());
            self.body.extend_from_slice(&buffer.as_ref()[..count]);
        }
        self.start_read(request);
    }

    fn on_succeeded(&mut self, _request: crate::RequestHandle, info: ResponseInfo) {
        let body = std::mem::take(&mut self.body);
        self.finish(Ok(Response { info, body }));
    }

    fn on_failed(
        &mut self,
        _request: crate::RequestHandle,
        _info: Option<ResponseInfo>,
        error: NetworkError,
    ) {
        self.finish(Err(RequestError::Network(error)));
    }

    fn on_canceled(&mut self, _request: crate::RequestHandle, _info: Option<ResponseInfo>) {
        self.finish(Err(RequestError::Canceled));
    }
}
