// ── ail-stdlib::net ───────────────────────────────────────────────────────
//
// Network primitive types for the AIL `std.net` module.
//
// # Capabilities (from docs/stdlib.md)
//
// - network.connect
// - http.call
//
// # Rules
//
// - network access requires grants
// - hosts/scopes can be constrained
// - timeouts explicit
// - retries explicit

// ── Url ───────────────────────────────────────────────────────────────────

/// A validated URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
    pub query: Option<String>,
}

impl Url {
    pub fn new(
        scheme: impl Into<String>,
        host: impl Into<String>,
        path: impl Into<String>,
    ) -> Self {
        Self {
            scheme: scheme.into(),
            host: host.into(),
            port: None,
            path: path.into(),
            query: None,
        }
    }

    /// Parse a URL string. Returns `Err` if malformed.
    pub fn parse(s: &str) -> Result<Self, NetError> {
        let (scheme, rest) = s
            .split_once("://")
            .ok_or_else(|| NetError::InvalidUrl(s.to_string()))?;
        let (authority, path_part) = rest.split_once('/').unwrap_or((rest, ""));
        let path = format!("/{path_part}");
        let (host_str, port_opt) = if let Some((h, p)) = authority.rsplit_once(':') {
            let port = p
                .parse::<u16>()
                .map_err(|_| NetError::InvalidUrl(s.to_string()))?;
            (h, Some(port))
        } else {
            (authority, None)
        };
        Ok(Self {
            scheme: scheme.to_string(),
            host: host_str.to_string(),
            port: port_opt,
            path,
            query: None,
        })
    }
}

impl std::fmt::Display for Url {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let port_part = self.port.map(|p| format!(":{p}")).unwrap_or_default();
        let query_part = self
            .query
            .as_deref()
            .map(|q| format!("?{q}"))
            .unwrap_or_default();
        write!(
            f,
            "{}://{}{}{}{}",
            self.scheme, self.host, port_part, self.path, query_part
        )
    }
}

// ── NetError ──────────────────────────────────────────────────────────────

/// Error from network operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetError {
    /// Network capability not granted.
    PermissionDenied,
    /// Connection timed out.
    Timeout,
    /// DNS resolution failed.
    DnsFailure(String),
    /// TCP/TLS connection failed.
    ConnectionFailed(String),
    /// Invalid URL.
    InvalidUrl(String),
    /// Other error.
    Other(String),
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::PermissionDenied => write!(f, "network permission denied"),
            NetError::Timeout => write!(f, "network timeout"),
            NetError::DnsFailure(h) => write!(f, "dns failure: {h}"),
            NetError::ConnectionFailed(msg) => write!(f, "connection failed: {msg}"),
            NetError::InvalidUrl(u) => write!(f, "invalid url: {u}"),
            NetError::Other(msg) => write!(f, "net error: {msg}"),
        }
    }
}
impl std::error::Error for NetError {}

// ── Timeout / RetryPolicy ─────────────────────────────────────────────────

/// An explicit timeout specification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeout {
    pub millis: u64,
}

impl Timeout {
    pub fn from_millis(ms: u64) -> Self {
        Self { millis: ms }
    }
    pub fn from_secs(s: u64) -> Self {
        Self { millis: s * 1000 }
    }
    pub fn none() -> Option<Self> {
        None
    }
}

/// An explicit retry policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff_millis: u64,
}

impl RetryPolicy {
    pub fn no_retry() -> Self {
        Self {
            max_attempts: 1,
            backoff_millis: 0,
        }
    }
    pub fn fixed(attempts: u32, backoff_ms: u64) -> Self {
        Self {
            max_attempts: attempts,
            backoff_millis: backoff_ms,
        }
    }
}
