use std::ffi::CStr;

use crate::sys;

/// One HTTP header.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Header {
    /// Header name.
    pub name: String,
    /// Header value.
    pub value: String,
}

/// A copy of response metadata supplied to a request callback.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResponseInfo {
    /// Final URL.
    pub url: String,
    /// Redirect URL chain.
    pub url_chain: Vec<String>,
    /// HTTP status code.
    pub status_code: i32,
    /// HTTP reason phrase.
    pub status_text: String,
    /// Response headers in wire order.
    pub headers: Vec<Header>,
    /// Whether the response came from cache.
    pub was_cached: bool,
    /// Negotiated protocol, such as `h2` or `h3`.
    pub negotiated_protocol: String,
    /// Proxy server description, if any.
    pub proxy_server: String,
    /// Bytes received at the time this snapshot was created.
    pub received_byte_count: i64,
}

impl ResponseInfo {
    pub(crate) unsafe fn copy_from(raw: sys::Cronet_UrlResponseInfoPtr) -> Option<Self> {
        if raw.is_null() {
            return None;
        }
        // SAFETY: Caller guarantees `raw` is valid for the callback. Every
        // string and child header is copied before returning.
        unsafe {
            let chain_len = sys::Cronet_UrlResponseInfo_url_chain_size(raw);
            let mut url_chain = Vec::with_capacity(chain_len as usize);
            for index in 0..chain_len {
                url_chain.push(copy_string(sys::Cronet_UrlResponseInfo_url_chain_at(
                    raw, index,
                )));
            }

            let header_len = sys::Cronet_UrlResponseInfo_all_headers_list_size(raw);
            let mut headers = Vec::with_capacity(header_len as usize);
            for index in 0..header_len {
                let header = sys::Cronet_UrlResponseInfo_all_headers_list_at(raw, index);
                if !header.is_null() {
                    headers.push(Header {
                        name: copy_string(sys::Cronet_HttpHeader_name_get(header)),
                        value: copy_string(sys::Cronet_HttpHeader_value_get(header)),
                    });
                }
            }

            Some(Self {
                url: copy_string(sys::Cronet_UrlResponseInfo_url_get(raw)),
                url_chain,
                status_code: sys::Cronet_UrlResponseInfo_http_status_code_get(raw),
                status_text: copy_string(sys::Cronet_UrlResponseInfo_http_status_text_get(raw)),
                headers,
                was_cached: sys::Cronet_UrlResponseInfo_was_cached_get(raw),
                negotiated_protocol: copy_string(
                    sys::Cronet_UrlResponseInfo_negotiated_protocol_get(raw),
                ),
                proxy_server: copy_string(sys::Cronet_UrlResponseInfo_proxy_server_get(raw)),
                received_byte_count: sys::Cronet_UrlResponseInfo_received_byte_count_get(raw),
            })
        }
    }
}

/// A copied network error reported by Cronet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NetworkError {
    /// Stable Cronet error category represented by its IDL numeric value.
    pub code: i32,
    /// Human-readable diagnostic.
    pub message: String,
    /// Chromium `net::Error` code.
    pub internal_error_code: i32,
    /// Whether retrying immediately may succeed.
    pub immediately_retryable: bool,
    /// Detailed QUIC error code, or zero when not applicable.
    pub quic_detailed_error_code: i32,
}

impl NetworkError {
    pub(crate) unsafe fn copy_from(raw: sys::Cronet_ErrorPtr) -> Self {
        // SAFETY: Callback guarantees the error object remains live while all
        // fields are copied.
        unsafe {
            Self {
                // Bindgen may represent this C enum as either i32 or u32,
                // depending on the target ABI. Cronet's public error codes
                // are non-negative and fit in i32.
                code: sys::Cronet_Error_error_code_get(raw) as i32,
                message: copy_string(sys::Cronet_Error_message_get(raw)),
                internal_error_code: sys::Cronet_Error_internal_error_code_get(raw),
                immediately_retryable: sys::Cronet_Error_immediately_retryable_get(raw),
                quic_detailed_error_code: sys::Cronet_Error_quic_detailed_error_code_get(raw),
            }
        }
    }
}

pub(crate) unsafe fn copy_string(raw: sys::Cronet_String) -> String {
    if raw.is_null() {
        return String::new();
    }
    // SAFETY: Caller guarantees a live NUL-terminated Cronet string.
    unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned()
}
