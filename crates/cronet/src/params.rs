use std::{
    ffi::{CStr, CString},
    ptr::NonNull,
};

use crate::{Executor, Header, UploadDataProvider, sys};

/// HTTP cache backend used by an [`EngineParams`] value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CacheMode {
    /// Disable the HTTP cache.
    Disabled,
    /// Keep cached responses in memory.
    InMemory,
    /// Use disk storage without caching HTTP response bodies.
    DiskNoHttp,
    /// Use the disk HTTP cache.
    Disk,
}

impl CacheMode {
    fn raw(self) -> sys::Cronet_EngineParams_HTTP_CACHE_MODE {
        match self {
            Self::Disabled => sys::Cronet_EngineParams_HTTP_CACHE_MODE_Cronet_EngineParams_HTTP_CACHE_MODE_DISABLED,
            Self::InMemory => sys::Cronet_EngineParams_HTTP_CACHE_MODE_Cronet_EngineParams_HTTP_CACHE_MODE_IN_MEMORY,
            Self::DiskNoHttp => sys::Cronet_EngineParams_HTTP_CACHE_MODE_Cronet_EngineParams_HTTP_CACHE_MODE_DISK_NO_HTTP,
            Self::Disk => sys::Cronet_EngineParams_HTTP_CACHE_MODE_Cronet_EngineParams_HTTP_CACHE_MODE_DISK,
        }
    }
}

/// Owned native engine configuration.
pub struct EngineParams(NonNull<sys::Cronet_EngineParams>);

impl EngineParams {
    /// Allocates a configuration with Cronet defaults.
    pub fn new() -> Self {
        // SAFETY: Native constructor takes no arguments.
        let ptr = unsafe { sys::Cronet_EngineParams_Create() };
        Self(NonNull::new(ptr).expect("Cronet_EngineParams_Create returned null"))
    }

    /// Overrides the HTTP user agent.
    pub fn user_agent(&mut self, value: &str) -> &mut Self {
        self.set_string(value, sys::Cronet_EngineParams_user_agent_set)
    }

    /// Sets the Accept-Language value.
    pub fn accept_language(&mut self, value: &str) -> &mut Self {
        self.set_string(value, sys::Cronet_EngineParams_accept_language_set)
    }

    /// Sets an existing directory used for persistent storage.
    pub fn storage_path(&mut self, value: &str) -> &mut Self {
        self.set_string(value, sys::Cronet_EngineParams_storage_path_set)
    }

    /// Enables or disables QUIC.
    pub fn enable_quic(&mut self, enabled: bool) -> &mut Self {
        // SAFETY: `self.0` is live and exclusively borrowed.
        unsafe { sys::Cronet_EngineParams_enable_quic_set(self.0.as_ptr(), enabled) };
        self
    }

    /// Enables or disables HTTP/2.
    pub fn enable_http2(&mut self, enabled: bool) -> &mut Self {
        // SAFETY: `self.0` is live and exclusively borrowed.
        unsafe { sys::Cronet_EngineParams_enable_http2_set(self.0.as_ptr(), enabled) };
        self
    }

    /// Enables or disables Brotli content decoding.
    pub fn enable_brotli(&mut self, enabled: bool) -> &mut Self {
        // SAFETY: `self.0` is live and exclusively borrowed.
        unsafe { sys::Cronet_EngineParams_enable_brotli_set(self.0.as_ptr(), enabled) };
        self
    }

    /// Configures the cache and maximum byte count.
    pub fn cache(&mut self, mode: CacheMode, max_size: i64) -> &mut Self {
        // SAFETY: `self.0` is live and exclusively borrowed.
        unsafe {
            sys::Cronet_EngineParams_http_cache_mode_set(self.0.as_ptr(), mode.raw());
            sys::Cronet_EngineParams_http_cache_max_size_set(self.0.as_ptr(), max_size);
        }
        self
    }

    /// Sets Cronet's JSON experimental options.
    pub fn experimental_options(&mut self, json: &str) -> &mut Self {
        self.set_string(json, sys::Cronet_EngineParams_experimental_options_set)
    }

    /// Merges one top-level Chromium experimental option.
    pub fn experimental_option(
        &mut self,
        key: &str,
        value: Option<serde_json::Value>,
    ) -> core::result::Result<&mut Self, serde_json::Error> {
        // SAFETY: Parameters are live and Cronet owns the returned C string.
        let raw = unsafe { sys::Cronet_EngineParams_experimental_options_get(self.0.as_ptr()) };
        let current = if raw.is_null() {
            ""
        } else {
            // SAFETY: Checked non-null Cronet string.
            unsafe { CStr::from_ptr(raw) }.to_str().unwrap_or("")
        };
        let mut options: serde_json::Map<String, serde_json::Value> = if current.trim().is_empty() {
            serde_json::Map::new()
        } else {
            serde_json::from_str(current)?
        };
        if let Some(value) = value {
            options.insert(key.to_owned(), value);
        } else {
            options.remove(key);
        }
        let encoded = serde_json::to_string(&options)?;
        Ok(self.experimental_options(&encoded))
    }

    /// Enables Chromium's asynchronous built-in DNS client.
    pub fn async_dns(
        &mut self,
        enabled: bool,
    ) -> core::result::Result<&mut Self, serde_json::Error> {
        self.experimental_option(
            "AsyncDNS",
            enabled.then(|| serde_json::json!({ "enable": true })),
        )
    }

    /// Overrides built-in DNS nameservers using `ip:port` strings.
    pub fn dns_server_override(
        &mut self,
        nameservers: &[String],
    ) -> core::result::Result<&mut Self, serde_json::Error> {
        self.experimental_option(
            "DnsServerOverride",
            (!nameservers.is_empty()).then(|| serde_json::json!({ "nameservers": nameservers })),
        )
    }

    /// Sets Chromium mapped-host-resolver rules.
    pub fn host_resolver_rules(
        &mut self,
        rules: &str,
    ) -> core::result::Result<&mut Self, serde_json::Error> {
        self.experimental_option(
            "HostResolverRules",
            (!rules.is_empty()).then(|| serde_json::json!({ "host_resolver_rules": rules })),
        )
    }

    /// Enables DNS HTTPS/SVCB queries required for ECH discovery.
    pub fn use_dns_https_svcb(
        &mut self,
        enabled: bool,
    ) -> core::result::Result<&mut Self, serde_json::Error> {
        self.experimental_option(
            "UseDnsHttpsSvcb",
            Some(serde_json::json!({ "enable": enabled })),
        )
    }

    /// Configures HTTP/2 flow-control receive windows.
    pub fn http2_windows(
        &mut self,
        session_max_receive_window: u64,
        initial_window: u64,
    ) -> core::result::Result<&mut Self, serde_json::Error> {
        self.experimental_option(
            "HTTP2Options",
            Some(serde_json::json!({
                "session_max_recv_window_size": session_max_receive_window,
                "initial_window_size": initial_window
            })),
        )
    }

    /// Configures QUIC connection options and receive windows.
    pub fn quic_options(
        &mut self,
        connection_options: &str,
        initial_stream_receive_window: u64,
        initial_session_receive_window: u64,
    ) -> core::result::Result<&mut Self, serde_json::Error> {
        let mut options = serde_json::Map::new();
        if !connection_options.is_empty() {
            options.insert("connection_options".into(), connection_options.into());
        }
        if initial_stream_receive_window != 0 {
            options.insert(
                "initial_stream_recv_window_size".into(),
                initial_stream_receive_window.into(),
            );
        }
        if initial_session_receive_window != 0 {
            options.insert(
                "initial_session_recv_window_size".into(),
                initial_session_receive_window.into(),
            );
        }
        let value = (!options.is_empty()).then(|| options.into());
        self.experimental_option("QUIC", value)
    }

    /// Raises Chromium socket-pool limits for proxy multiplexing.
    pub fn socket_pool_limits(
        &mut self,
        per_pool: usize,
        per_proxy_chain: usize,
        per_group: usize,
    ) -> core::result::Result<&mut Self, serde_json::Error> {
        self.experimental_option(
            "SocketPoolOptions",
            Some(serde_json::json!({
                "max_sockets_per_pool": per_pool,
                "max_sockets_per_proxy_chain": per_proxy_chain,
                "max_sockets_per_group": per_group
            })),
        )
    }

    /// Enables additional native argument and state validation.
    pub fn enable_check_result(&mut self, enabled: bool) -> &mut Self {
        // SAFETY: `self.0` is live and exclusively borrowed.
        unsafe { sys::Cronet_EngineParams_enable_check_result_set(self.0.as_ptr(), enabled) };
        self
    }

    /// Sets the relative priority of Cronet's network thread.
    pub fn network_thread_priority(&mut self, priority: f64) -> &mut Self {
        // SAFETY: `self.0` is live and exclusively borrowed.
        unsafe { sys::Cronet_EngineParams_network_thread_priority_set(self.0.as_ptr(), priority) };
        self
    }

    /// Controls pinning bypass for locally trusted certificate authorities.
    pub fn bypass_pinning_for_local_trust_anchors(&mut self, enabled: bool) -> &mut Self {
        // SAFETY: `self.0` is live and exclusively borrowed.
        unsafe {
            sys::Cronet_EngineParams_enable_public_key_pinning_bypass_for_local_trust_anchors_set(
                self.0.as_ptr(),
                enabled,
            )
        };
        self
    }

    /// Adds a QUIC alternate-service hint.
    pub fn quic_hint(&mut self, host: &str, port: i32, alternate_port: i32) -> &mut Self {
        let host = CString::new(host).expect("QUIC host cannot contain NUL bytes");
        // SAFETY: Generated array setter copies the temporary struct.
        unsafe {
            let hint = sys::Cronet_QuicHint_Create();
            let hint = NonNull::new(hint).expect("Cronet_QuicHint_Create returned null");
            sys::Cronet_QuicHint_host_set(hint.as_ptr(), host.as_ptr());
            sys::Cronet_QuicHint_port_set(hint.as_ptr(), port);
            sys::Cronet_QuicHint_alternate_port_set(hint.as_ptr(), alternate_port);
            sys::Cronet_EngineParams_quic_hints_add(self.0.as_ptr(), hint.as_ptr());
            sys::Cronet_QuicHint_Destroy(hint.as_ptr());
        }
        self
    }

    /// Adds a public-key pin set. Pins are base64-encoded SHA-256 SPKI hashes.
    pub fn public_key_pins(
        &mut self,
        host: &str,
        pins_sha256: &[String],
        include_subdomains: bool,
        expiration_unix_millis: i64,
    ) -> &mut Self {
        let host = CString::new(host).expect("pin host cannot contain NUL bytes");
        // SAFETY: Generated array setters copy strings and the temporary struct.
        unsafe {
            let pins = sys::Cronet_PublicKeyPins_Create();
            let pins = NonNull::new(pins).expect("Cronet_PublicKeyPins_Create returned null");
            sys::Cronet_PublicKeyPins_host_set(pins.as_ptr(), host.as_ptr());
            for pin in pins_sha256 {
                let pin = CString::new(pin.as_str()).expect("pin cannot contain NUL bytes");
                sys::Cronet_PublicKeyPins_pins_sha256_add(pins.as_ptr(), pin.as_ptr());
            }
            sys::Cronet_PublicKeyPins_include_subdomains_set(pins.as_ptr(), include_subdomains);
            sys::Cronet_PublicKeyPins_expiration_date_set(pins.as_ptr(), expiration_unix_millis);
            sys::Cronet_EngineParams_public_key_pins_add(self.0.as_ptr(), pins.as_ptr());
            sys::Cronet_PublicKeyPins_Destroy(pins.as_ptr());
        }
        self
    }

    pub(crate) fn as_ptr(&self) -> sys::Cronet_EngineParamsPtr {
        self.0.as_ptr()
    }

    fn set_string(
        &mut self,
        value: &str,
        setter: unsafe extern "C" fn(sys::Cronet_EngineParamsPtr, sys::Cronet_String),
    ) -> &mut Self {
        let value = CString::new(value).expect("Cronet strings cannot contain NUL bytes");
        // SAFETY: Pointers are valid for the call; generated struct setters copy strings.
        unsafe { setter(self.0.as_ptr(), value.as_ptr()) };
        self
    }
}

impl Default for EngineParams {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for EngineParams {
    fn drop(&mut self) {
        // SAFETY: This wrapper uniquely owns the native object.
        unsafe { sys::Cronet_EngineParams_Destroy(self.0.as_ptr()) };
    }
}

/// Scheduling priority for a URL request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestPriority {
    /// Background work that may be delayed.
    Idle,
    /// Lowest non-idle priority.
    Lowest,
    /// Low priority.
    Low,
    /// Default priority.
    Medium,
    /// Highest priority.
    Highest,
}

impl RequestPriority {
    fn raw(self) -> sys::Cronet_UrlRequestParams_REQUEST_PRIORITY {
        match self {
            Self::Idle => 0,
            Self::Lowest => 1,
            Self::Low => 2,
            Self::Medium => 3,
            Self::Highest => 4,
        }
    }
}

/// Owned native URL request configuration.
pub struct RequestParams(NonNull<sys::Cronet_UrlRequestParams>);

impl RequestParams {
    /// Allocates request parameters with Cronet defaults.
    pub fn new() -> Self {
        // SAFETY: Native constructor takes no arguments.
        let ptr = unsafe { sys::Cronet_UrlRequestParams_Create() };
        Self(NonNull::new(ptr).expect("Cronet_UrlRequestParams_Create returned null"))
    }

    /// Sets the HTTP method.
    pub fn method(&mut self, method: &str) -> &mut Self {
        let method = CString::new(method).expect("HTTP method cannot contain NUL bytes");
        // SAFETY: Generated setter copies the string.
        unsafe { sys::Cronet_UrlRequestParams_http_method_set(self.0.as_ptr(), method.as_ptr()) };
        self
    }

    /// Controls use of the HTTP cache for this request.
    pub fn disable_cache(&mut self, disabled: bool) -> &mut Self {
        // SAFETY: `self.0` is live and exclusively borrowed.
        unsafe { sys::Cronet_UrlRequestParams_disable_cache_set(self.0.as_ptr(), disabled) };
        self
    }

    /// Sets request scheduling priority.
    pub fn priority(&mut self, priority: RequestPriority) -> &mut Self {
        // SAFETY: `self.0` is live and exclusively borrowed.
        unsafe { sys::Cronet_UrlRequestParams_priority_set(self.0.as_ptr(), priority.raw()) };
        self
    }

    /// Allows callbacks on Cronet's network thread.
    ///
    /// Use this only when every callback is non-blocking and audited.
    pub fn allow_direct_executor(&mut self, allowed: bool) -> &mut Self {
        // SAFETY: `self.0` is live and exclusively borrowed.
        unsafe { sys::Cronet_UrlRequestParams_allow_direct_executor_set(self.0.as_ptr(), allowed) };
        self
    }

    /// Appends a request header.
    pub fn header(&mut self, header: Header) -> &mut Self {
        let name = CString::new(header.name).expect("header name cannot contain NUL bytes");
        let value = CString::new(header.value).expect("header value cannot contain NUL bytes");
        // SAFETY: The temporary native header is live for every call. Generated
        // array setters copy struct values, as required by the Cronet C API.
        unsafe {
            let raw = sys::Cronet_HttpHeader_Create();
            let raw = NonNull::new(raw).expect("Cronet_HttpHeader_Create returned null");
            sys::Cronet_HttpHeader_name_set(raw.as_ptr(), name.as_ptr());
            sys::Cronet_HttpHeader_value_set(raw.as_ptr(), value.as_ptr());
            sys::Cronet_UrlRequestParams_request_headers_add(self.0.as_ptr(), raw.as_ptr());
            sys::Cronet_HttpHeader_Destroy(raw.as_ptr());
        }
        self
    }

    pub(crate) fn as_ptr(&self) -> sys::Cronet_UrlRequestParamsPtr {
        self.0.as_ptr()
    }

    /// Returns the native pointer for APIs not yet lifted into this wrapper.
    pub fn as_raw(&self) -> sys::Cronet_UrlRequestParamsPtr {
        self.0.as_ptr()
    }

    pub(crate) fn upload(
        &mut self,
        provider: &UploadDataProvider,
        executor: &Executor,
    ) -> &mut Self {
        // SAFETY: Client keeps both resources alive through request completion.
        unsafe {
            sys::Cronet_UrlRequestParams_upload_data_provider_set(
                self.0.as_ptr(),
                provider.as_raw(),
            );
            sys::Cronet_UrlRequestParams_upload_data_provider_executor_set(
                self.0.as_ptr(),
                executor.as_raw(),
            );
        }
        self
    }
}

impl Default for RequestParams {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RequestParams {
    fn drop(&mut self) {
        // SAFETY: This wrapper uniquely owns the native object.
        unsafe { sys::Cronet_UrlRequestParams_Destroy(self.0.as_ptr()) };
    }
}
