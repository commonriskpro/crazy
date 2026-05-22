// ── ail-stdlib::http ──────────────────────────────────────────────────────
//
// HTTP client/server types for the AIL `std.http` module.
//
// # Capabilities
//
// - http.call
//
// # Rules (from docs/stdlib.md)
//
// - network access requires grants
// - timeouts explicit
// - retries explicit

use std::collections::BTreeMap;

// ── HeaderMap ─────────────────────────────────────────────────────────────

/// An ordered map of HTTP header names to values.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct HeaderMap(pub BTreeMap<String, String>);

impl HeaderMap {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0.insert(key.into().to_ascii_lowercase(), value.into());
    }
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(&key.to_ascii_lowercase()).map(String::as_str)
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// ── StatusCode ────────────────────────────────────────────────────────────

/// An HTTP status code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct StatusCode(pub u16);

impl StatusCode {
    pub fn ok() -> Self {
        Self(200)
    }
    pub fn not_found() -> Self {
        Self(404)
    }
    pub fn internal_server_error() -> Self {
        Self(500)
    }
    pub fn is_success(self) -> bool {
        self.0 >= 200 && self.0 < 300
    }
    pub fn is_client_error(self) -> bool {
        self.0 >= 400 && self.0 < 500
    }
    pub fn is_server_error(self) -> bool {
        self.0 >= 500
    }
}

impl std::fmt::Display for StatusCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── HttpMethod ────────────────────────────────────────────────────────────

/// HTTP method.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
    Custom(String),
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpMethod::Get => write!(f, "GET"),
            HttpMethod::Post => write!(f, "POST"),
            HttpMethod::Put => write!(f, "PUT"),
            HttpMethod::Patch => write!(f, "PATCH"),
            HttpMethod::Delete => write!(f, "DELETE"),
            HttpMethod::Head => write!(f, "HEAD"),
            HttpMethod::Options => write!(f, "OPTIONS"),
            HttpMethod::Custom(s) => write!(f, "{s}"),
        }
    }
}

// ── HttpRequest ───────────────────────────────────────────────────────────

/// An HTTP request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: HeaderMap,
    pub body: Option<Vec<u8>>,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Get,
            url: url.into(),
            headers: HeaderMap::new(),
            body: None,
        }
    }

    pub fn post(url: impl Into<String>, body: Vec<u8>) -> Self {
        Self {
            method: HttpMethod::Post,
            url: url.into(),
            headers: HeaderMap::new(),
            body: Some(body),
        }
    }

    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key, value);
        self
    }
}

// ── HttpResponse ──────────────────────────────────────────────────────────

/// An HTTP response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn new(status: StatusCode, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: HeaderMap::new(),
            body,
        }
    }

    pub fn body_as_text(&self) -> Result<String, String> {
        std::str::from_utf8(&self.body)
            .map(str::to_string)
            .map_err(|e| e.to_string())
    }
}

// ── HttpError ─────────────────────────────────────────────────────────────

/// Error from HTTP operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpError {
    PermissionDenied,
    Timeout,
    ConnectionFailed(String),
    InvalidRequest(String),
    ServerError(StatusCode),
    Other(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::PermissionDenied => write!(f, "http permission denied"),
            HttpError::Timeout => write!(f, "http timeout"),
            HttpError::ConnectionFailed(msg) => write!(f, "connection failed: {msg}"),
            HttpError::InvalidRequest(msg) => write!(f, "invalid request: {msg}"),
            HttpError::ServerError(c) => write!(f, "server error: {c}"),
            HttpError::Other(msg) => write!(f, "http error: {msg}"),
        }
    }
}
impl std::error::Error for HttpError {}
