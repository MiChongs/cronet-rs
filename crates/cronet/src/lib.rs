//! Safe ownership primitives for Chromium Cronet.
//!
//! The crate intentionally keeps the complete generated ABI available as
//! [`sys`]. Safe wrappers are layered over that ABI without hiding new symbols
//! introduced by later Chromium releases.

mod async_client;
mod bidirectional;
mod bidirectional_conn;
mod buffer;
mod callback;
mod client;
mod engine;
mod error;
mod executor;
mod metrics;
mod model;
mod naive;
mod naive_client;
mod params;
mod request;
mod status;
mod upload;

pub use buffer::Buffer;
pub use callback::{RequestHandle, UrlRequestCallback, UrlRequestHandler};
pub use client::{Client, RequestError, RequestOptions, Response};
pub use engine::{Engine, NetworkHooks, UdpDialResult};
pub use error::{Error, Result};
pub use executor::{Executor, Runnable};
pub use metrics::{
    FinishedReason, Metrics, RequestFinished, RequestFinishedInfo, RequestFinishedListener,
};
pub use model::{Header, NetworkError, ResponseInfo};
pub use naive::{NaiveConnectOptions, NaiveConnection};
pub use naive_client::{NaiveClient, NaiveClientOptions, NaiveClientStartError};
pub use params::{CacheMode, EngineParams, RequestParams, RequestPriority};
pub use request::{Request, UninitializedRequest};
pub use status::RequestStatus;
pub use upload::UploadDataProvider;

pub use async_client::AsyncClient;
pub use bidirectional::{
    BidirectionalStream, BidirectionalStreamHandle, BidirectionalStreamHandler,
};
pub use bidirectional_conn::BidirectionalConnection;
/// The complete, SDK-version-matched native Cronet API.
pub use cronet_sys as sys;
