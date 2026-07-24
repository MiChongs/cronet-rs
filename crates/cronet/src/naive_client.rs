use std::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::{
    Engine, EngineParams, Error as CronetError, Header, NaiveConnectOptions, NaiveConnection,
    NetworkHooks, dns,
};

/// Failure while configuring or starting a [`NaiveClient`].
#[derive(Debug)]
pub enum NaiveClientStartError {
    /// Cronet rejected engine startup.
    Cronet(CronetError),
    /// Existing experimental-options JSON was malformed.
    ExperimentalOptions(serde_json::Error),
    /// The proxy URL could not be parsed.
    InvalidProxyUrl(url::ParseError),
    /// The proxy URL has no DNS host and no explicit `server_name`.
    MissingServerName,
    /// Multiple independent pools are unsupported by upstream in QUIC mode.
    InsecureConcurrencyWithQuic,
}

impl fmt::Display for NaiveClientStartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cronet(error) => error.fmt(formatter),
            Self::ExperimentalOptions(error) => error.fmt(formatter),
            Self::InvalidProxyUrl(error) => error.fmt(formatter),
            Self::MissingServerName => formatter.write_str(
                "proxy URL has no DNS host; set NaiveClientOptions::server_name explicitly",
            ),
            Self::InsecureConcurrencyWithQuic => {
                formatter.write_str("insecure concurrency is not supported with QUIC")
            }
        }
    }
}

impl std::error::Error for NaiveClientStartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Cronet(error) => Some(error),
            Self::ExperimentalOptions(error) => Some(error),
            Self::InvalidProxyUrl(error) => Some(error),
            Self::MissingServerName => None,
            Self::InsecureConcurrencyWithQuic => None,
        }
    }
}

impl From<CronetError> for NaiveClientStartError {
    fn from(error: CronetError) -> Self {
        Self::Cronet(error)
    }
}

impl From<serde_json::Error> for NaiveClientStartError {
    fn from(error: serde_json::Error) -> Self {
        Self::ExperimentalOptions(error)
    }
}

impl From<url::ParseError> for NaiveClientStartError {
    fn from(error: url::ParseError) -> Self {
        Self::InvalidProxyUrl(error)
    }
}

/// Configuration shared by all tunnels opened by a [`NaiveClient`].
#[derive(Clone, Debug)]
pub struct NaiveClientOptions {
    /// HTTPS URL of the NaiveProxy server.
    pub proxy_url: String,
    /// Username used to create a Basic proxy authorization value.
    pub username: Option<String>,
    /// Password used with `username`.
    pub password: Option<String>,
    /// Precomputed `Proxy-Authorization` value. This takes precedence over
    /// `username` and `password`.
    pub proxy_authorization: Option<String>,
    /// Number of independent Chromium connection pools.
    pub insecure_concurrency: usize,
    /// Headers included with every CONNECT stream.
    pub extra_headers: Vec<Header>,
    /// Enables the SagerNet Cronet HTTP/3 path.
    pub quic: bool,
    /// Enables HTTPS/SVCB DNS processing required for ECH.
    pub ech_enabled: bool,
    /// TLS server name used for HTTPS/SVCB matching. By default this is
    /// derived from `proxy_url`.
    pub server_name: Option<String>,
    /// Network address resolved for `server_name`. This may be an IP literal
    /// or a different DNS name.
    pub server_address: Option<String>,
    /// Fixed binary ECHConfigList injected into matching HTTPS answers.
    pub ech_config_list: Vec<u8>,
    /// Optional alternate name queried for HTTPS records before answers are
    /// rewritten to `server_name`.
    pub ech_query_server_name: Option<String>,
    /// QUIC connection option, for example `TBBR`, `B2ON`, `QBIC` or `RENO`.
    pub quic_congestion_control: String,
    /// Initial per-stream receive window. Zero selects the upstream default.
    pub receive_window: u64,
    /// Initial QUIC session receive window. Zero selects the upstream default.
    pub quic_session_receive_window: u64,
    /// Buffer size used by each asynchronous native stream read.
    pub read_buffer_size: usize,
    /// Native bidirectional-stream priority.
    pub priority: i32,
}

impl NaiveClientOptions {
    /// Creates options for a NaiveProxy server URL.
    pub fn new(proxy_url: impl Into<String>) -> Self {
        Self {
            proxy_url: proxy_url.into(),
            username: None,
            password: None,
            proxy_authorization: None,
            insecure_concurrency: 1,
            extra_headers: Vec::new(),
            quic: false,
            ech_enabled: false,
            server_name: None,
            server_address: None,
            ech_config_list: Vec::new(),
            ech_query_server_name: None,
            quic_congestion_control: String::new(),
            receive_window: 0,
            quic_session_receive_window: 0,
            read_buffer_size: 32 * 1024,
            priority: 0,
        }
    }
}

/// Started SagerNet Cronet engine configured for NaiveProxy tunnels.
///
/// Connections borrow this client, preventing engine shutdown while a native
/// stream remains live.
pub struct NaiveClient {
    engine: Engine,
    options: NaiveClientOptions,
    authorization: Option<String>,
    counter: AtomicU64,
}

impl NaiveClient {
    /// Starts a client with the same H2/H3 window and pool defaults used by
    /// `cronet-go`.
    pub fn start(
        options: NaiveClientOptions,
        hooks: NetworkHooks,
    ) -> std::result::Result<Self, NaiveClientStartError> {
        let mut params = EngineParams::new();
        params.enable_check_result(true);
        params.enable_brotli(true);
        params.socket_pool_limits(2048, 2048, 2040)?;
        if hooks.has_dns_resolver() {
            params.async_dns(true)?;
            params.dns_server_override(&["127.0.0.1:53".to_owned()])?;
            params.use_dns_https_svcb(options.ech_enabled)?;
        }

        if options.quic {
            params.enable_quic(true);
            let stream_window = nonzero_or(options.receive_window, 6 * 1024 * 1024);
            let session_window = nonzero_or(options.quic_session_receive_window, 15 * 1024 * 1024);
            params.quic_options(
                &options.quic_congestion_control,
                stream_window,
                session_window,
            )?;
        } else {
            params.enable_quic(false);
            params.enable_http2(true);
            let receive_window = nonzero_or(options.receive_window, 128 * 1024 * 1024);
            params.http2_windows(receive_window, receive_window / 2)?;
        }

        Self::start_with_params(options, hooks, &params)
    }

    /// Starts a client with caller-supplied Cronet engine parameters.
    pub fn start_with_params(
        mut options: NaiveClientOptions,
        mut hooks: NetworkHooks,
        params: &EngineParams,
    ) -> std::result::Result<Self, NaiveClientStartError> {
        options.insecure_concurrency = options.insecure_concurrency.max(1);
        if options.quic && options.insecure_concurrency > 1 {
            return Err(NaiveClientStartError::InsecureConcurrencyWithQuic);
        }
        let proxy_url = url::Url::parse(&options.proxy_url)?;
        let proxy_host = proxy_url
            .host_str()
            .map(ToOwned::to_owned)
            .ok_or(NaiveClientStartError::MissingServerName)?;
        let server_name = options
            .server_name
            .clone()
            .unwrap_or_else(|| proxy_host.clone());
        let server_address = options
            .server_address
            .clone()
            .unwrap_or_else(|| proxy_host.clone());
        if hooks.has_dns_resolver() && !server_name.eq_ignore_ascii_case(&server_address) {
            hooks.configure_server_redirect(server_name.clone(), server_address);
        }
        if options.ech_enabled && hooks.has_dns_resolver() {
            let query_server_name = options
                .ech_query_server_name
                .clone()
                .unwrap_or_else(|| server_name.clone());
            hooks.configure_ech(dns::EchOptions {
                server_name,
                query_server_name,
                config_list: options.ech_config_list.clone(),
                quic: options.quic,
            });
        }
        let authorization = options.proxy_authorization.clone().or_else(|| {
            options.username.as_ref().map(|username| {
                let password = options.password.as_deref().unwrap_or_default();
                format!(
                    "Basic {}",
                    STANDARD.encode(format!("{username}:{password}"))
                )
            })
        });
        let engine = Engine::start_with_network_hooks(params, hooks)?;
        Ok(Self {
            engine,
            options,
            authorization,
            counter: AtomicU64::new(0),
        })
    }

    /// Returns the underlying started Cronet engine.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Starts a CONNECT stream without waiting for its response headers.
    pub fn dial_early(
        &self,
        destination: impl Into<String>,
    ) -> std::io::Result<NaiveConnection<'_>> {
        let pool = if self.options.insecure_concurrency > 1 {
            let index = self.counter.fetch_add(1, Ordering::Relaxed)
                % self.options.insecure_concurrency as u64;
            Some(format!("https://pool-{index}:443"))
        } else {
            None
        };
        self.engine.dial_naive(NaiveConnectOptions {
            proxy_url: self.options.proxy_url.clone(),
            destination: destination.into(),
            proxy_authorization: self.authorization.clone(),
            extra_headers: self.options.extra_headers.clone(),
            force_quic: self.options.quic,
            network_isolation_key: pool,
            priority: self.options.priority,
            read_buffer_size: self.options.read_buffer_size,
        })
    }

    /// Opens a CONNECT stream and completes the Naive handshake.
    pub fn dial(&self, destination: impl Into<String>) -> std::io::Result<NaiveConnection<'_>> {
        let connection = self.dial_early(destination)?;
        connection.handshake()?;
        Ok(connection)
    }

    /// Closes all pooled connections and shuts down the engine.
    ///
    /// Consuming `self` makes this unavailable while returned connections still
    /// borrow the client.
    pub fn shutdown(self) {
        self.engine.close_all_connections();
    }
}

fn nonzero_or(value: u64, fallback: u64) -> u64 {
    if value == 0 { fallback } else { value }
}

#[cfg(test)]
mod tests {
    use super::{NaiveClientOptions, nonzero_or};

    #[test]
    fn defaults_match_upstream_pool_behavior() {
        let options = NaiveClientOptions::new("https://proxy.example:443");
        assert_eq!(options.insecure_concurrency, 1);
        assert!(!options.quic);
        assert_eq!(nonzero_or(0, 42), 42);
        assert_eq!(nonzero_or(7, 42), 7);
    }
}
