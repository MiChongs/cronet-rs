use std::{
    ptr::NonNull,
    sync::mpsc::{self, Receiver, SyncSender},
};

use crate::sys;

/// Current phase of a Cronet URL request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestStatus {
    /// No active network operation.
    Idle,
    /// Waiting for a stalled socket pool.
    WaitingForStalledSocketPool,
    /// Waiting for an available socket.
    WaitingForAvailableSocket,
    /// Waiting for a network delegate.
    WaitingForDelegate,
    /// Waiting for the cache.
    WaitingForCache,
    /// Downloading a proxy auto-configuration file.
    DownloadingPacFile,
    /// Resolving a proxy.
    ResolvingProxyForUrl,
    /// Resolving a host from PAC.
    ResolvingHostInPacFile,
    /// Establishing a proxy tunnel.
    EstablishingProxyTunnel,
    /// Resolving the origin host.
    ResolvingHost,
    /// Connecting.
    Connecting,
    /// Performing a TLS handshake.
    SslHandshake,
    /// Sending request data.
    SendingRequest,
    /// Waiting for response headers.
    WaitingForResponse,
    /// Reading the response body.
    ReadingResponse,
    /// SDK-specific or invalid status value.
    Unknown(i32),
}

impl RequestStatus {
    fn from_raw(value: sys::Cronet_UrlRequestStatusListener_Status) -> Self {
        match value {
            0 => Self::Idle,
            1 => Self::WaitingForStalledSocketPool,
            2 => Self::WaitingForAvailableSocket,
            3 => Self::WaitingForDelegate,
            4 => Self::WaitingForCache,
            5 => Self::DownloadingPacFile,
            6 => Self::ResolvingProxyForUrl,
            7 => Self::ResolvingHostInPacFile,
            8 => Self::EstablishingProxyTunnel,
            9 => Self::ResolvingHost,
            10 => Self::Connecting,
            11 => Self::SslHandshake,
            12 => Self::SendingRequest,
            13 => Self::WaitingForResponse,
            14 => Self::ReadingResponse,
            other => Self::Unknown(other),
        }
    }
}

struct StatusState {
    sender: SyncSender<RequestStatus>,
}

pub(crate) fn request_status(request: sys::Cronet_UrlRequestPtr) -> Receiver<RequestStatus> {
    let (sender, receiver) = mpsc::sync_channel(1);
    let state = NonNull::from(Box::leak(Box::new(StatusState { sender })));
    // SAFETY: Trampoline has the generated C signature.
    let listener = unsafe { sys::Cronet_UrlRequestStatusListener_CreateWith(Some(on_status)) };
    let listener =
        NonNull::new(listener).expect("Cronet_UrlRequestStatusListener_CreateWith returned null");
    // SAFETY: Listener and request are live; callback takes responsibility for
    // destroying both the listener context and listener object.
    unsafe {
        sys::Cronet_UrlRequestStatusListener_SetClientContext(
            listener.as_ptr(),
            state.as_ptr().cast(),
        );
        sys::Cronet_UrlRequest_GetStatus(request, listener.as_ptr());
    }
    receiver
}

unsafe extern "C" fn on_status(
    listener: sys::Cronet_UrlRequestStatusListenerPtr,
    status: sys::Cronet_UrlRequestStatusListener_Status,
) {
    // SAFETY: Context was installed immediately before GetStatus.
    let context = unsafe { sys::Cronet_UrlRequestStatusListener_GetClientContext(listener) }
        .cast::<StatusState>();
    if !context.is_null() {
        // SAFETY: This callback is invoked once and uniquely reclaims context.
        let state = unsafe { Box::from_raw(context) };
        let _ = state.sender.send(RequestStatus::from_raw(status));
    }
    // SAFETY: One-shot listener is no longer used after this callback.
    unsafe {
        sys::Cronet_UrlRequestStatusListener_SetClientContext(listener, core::ptr::null_mut());
        sys::Cronet_UrlRequestStatusListener_Destroy(listener);
    }
}
