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

    /// Insert a header only after validating the redacted header contract.
    pub fn insert_checked(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), HttpHeaderShapeError> {
        let key = key.into();
        let value = value.into();
        let descriptor = HttpHeaderDescriptor::new(&key, &value);
        descriptor.validate()?;
        self.insert(key, value);
        Ok(())
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

// ── Http capability / request descriptors ────────────────────────────────

/// Enumeration of HTTP capabilities required by std.http operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpCapability {
    Call,
}

impl HttpCapability {
    /// Stable runtime capability label required by the host boundary.
    pub fn label(self) -> &'static str {
        match self {
            HttpCapability::Call => "http.call",
        }
    }
}

/// Stable structural categories for HTTP header names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpHeaderNameShape {
    Empty,
    ContainsColon,
    ContainsWhitespace,
    ContainsControl,
    NonAscii,
    Token,
}

impl HttpHeaderNameShape {
    pub fn label(self) -> &'static str {
        match self {
            HttpHeaderNameShape::Empty => "empty",
            HttpHeaderNameShape::ContainsColon => "contains-colon",
            HttpHeaderNameShape::ContainsWhitespace => "contains-whitespace",
            HttpHeaderNameShape::ContainsControl => "contains-control",
            HttpHeaderNameShape::NonAscii => "non-ascii",
            HttpHeaderNameShape::Token => "token",
        }
    }

    pub fn is_allowed(self) -> bool {
        matches!(self, HttpHeaderNameShape::Token)
    }
}

/// Stable structural categories for HTTP header values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpHeaderValueShape {
    Empty,
    Present,
    ContainsLineBreak,
    ContainsNul,
}

impl HttpHeaderValueShape {
    pub fn label(self) -> &'static str {
        match self {
            HttpHeaderValueShape::Empty => "empty",
            HttpHeaderValueShape::Present => "present",
            HttpHeaderValueShape::ContainsLineBreak => "contains-line-break",
            HttpHeaderValueShape::ContainsNul => "contains-nul",
        }
    }

    pub fn is_allowed(self) -> bool {
        !matches!(
            self,
            HttpHeaderValueShape::ContainsLineBreak | HttpHeaderValueShape::ContainsNul
        )
    }
}

/// Redacted error for invalid HTTP header shapes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpHeaderShapeError {
    pub field: &'static str,
    pub actual: &'static str,
    pub expected: &'static str,
}

impl std::fmt::Display for HttpHeaderShapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "http header shape mismatch for {}: expected {}, got {}",
            self.field, self.expected, self.actual
        )
    }
}

impl std::error::Error for HttpHeaderShapeError {}

/// Descriptor for a single HTTP header entry. Values/names are never echoed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpHeaderDescriptor {
    pub name_shape: HttpHeaderNameShape,
    pub value_shape: HttpHeaderValueShape,
    pub canonical_name_required: bool,
}

impl HttpHeaderDescriptor {
    pub fn new(name: &str, value: &str) -> Self {
        Self {
            name_shape: header_name_shape(name),
            value_shape: header_value_shape(value),
            canonical_name_required: true,
        }
    }

    pub fn diagnostic_key(&self) -> String {
        format!(
            "std.http.header:{}:{}",
            self.name_shape.label(),
            self.value_shape.label()
        )
    }

    pub fn validate(&self) -> Result<(), HttpHeaderShapeError> {
        validate_header_name_shape(self.name_shape)?;
        validate_header_value_shape(self.value_shape)
    }
}

/// Stable structural categories for request URLs that avoid leaking full URLs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpUrlShape {
    Empty,
    MissingScheme,
    UnsupportedScheme,
    MissingHost,
    ContainsCredentials,
    Http,
    Https,
}

impl HttpUrlShape {
    /// Diagnostic label that does not include the URL itself.
    pub fn label(self) -> &'static str {
        match self {
            HttpUrlShape::Empty => "empty",
            HttpUrlShape::MissingScheme => "missing-scheme",
            HttpUrlShape::UnsupportedScheme => "unsupported-scheme",
            HttpUrlShape::MissingHost => "missing-host",
            HttpUrlShape::ContainsCredentials => "contains-credentials",
            HttpUrlShape::Http => "http",
            HttpUrlShape::Https => "https",
        }
    }

    /// Whether this URL shape can reach the host HTTP boundary.
    pub fn is_allowed(self) -> bool {
        matches!(self, HttpUrlShape::Http | HttpUrlShape::Https)
    }
}

/// Stable structural categories for HTTP method tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpMethodShape {
    Standard,
    CustomToken,
    Empty,
    ContainsWhitespace,
    ContainsControl,
}

impl HttpMethodShape {
    /// Diagnostic label that does not expose custom method text.
    pub fn label(self) -> &'static str {
        match self {
            HttpMethodShape::Standard => "standard",
            HttpMethodShape::CustomToken => "custom-token",
            HttpMethodShape::Empty => "empty",
            HttpMethodShape::ContainsWhitespace => "contains-whitespace",
            HttpMethodShape::ContainsControl => "contains-control",
        }
    }

    /// Whether this method shape can reach the host HTTP boundary.
    pub fn is_allowed(self) -> bool {
        matches!(
            self,
            HttpMethodShape::Standard | HttpMethodShape::CustomToken
        )
    }
}

/// Stable body categories for request diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpBodyShape {
    Empty,
    Present,
}

impl HttpBodyShape {
    pub fn label(self) -> &'static str {
        match self {
            HttpBodyShape::Empty => "empty-body",
            HttpBodyShape::Present => "body",
        }
    }
}

/// Error produced when a request has an unsupported structural contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequestShapeError {
    pub field: &'static str,
    pub actual: &'static str,
    pub expected: &'static str,
}

impl std::fmt::Display for HttpRequestShapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "http request shape mismatch for {}: expected {}, got {}",
            self.field, self.expected, self.actual
        )
    }
}

impl std::error::Error for HttpRequestShapeError {}

/// Descriptor proving std.http requests are capability-gated, not ambient.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequestDescriptor {
    pub capability: HttpCapability,
    pub capability_label: &'static str,
    pub method_label: &'static str,
    pub method_shape: HttpMethodShape,
    pub url_shape: HttpUrlShape,
    pub body_shape: HttpBodyShape,
    pub grant_required: bool,
    pub ambient_access: bool,
    pub timeout_required: bool,
    pub retry_policy_explicit: bool,
}

impl HttpRequestDescriptor {
    /// Build a descriptor for a request without granting network access.
    pub fn new(request: &HttpRequest) -> Self {
        let capability = HttpCapability::Call;
        Self {
            capability,
            capability_label: capability.label(),
            method_label: method_diagnostic_label(&request.method),
            method_shape: method_shape(&request.method),
            url_shape: url_shape(&request.url),
            body_shape: body_shape(request),
            grant_required: true,
            ambient_access: false,
            timeout_required: true,
            retry_policy_explicit: true,
        }
    }

    /// Deterministic low-cardinality descriptor suitable for logs/registries.
    pub fn diagnostic_key(&self) -> String {
        format!(
            "std.http.request:{}:{}:{}:{}",
            self.capability_label,
            self.url_shape.label(),
            self.method_label,
            self.body_shape.label()
        )
    }

    /// Validate request shape before any host HTTP operation runs.
    pub fn validate_request_shape(&self) -> Result<(), HttpRequestShapeError> {
        validate_method_shape(self.method_shape)?;
        validate_url_shape(self.url_shape)
    }
}

/// Return the stable structural shape for a header name without exposing it.
pub fn header_name_shape(name: &str) -> HttpHeaderNameShape {
    if name.is_empty() {
        return HttpHeaderNameShape::Empty;
    }
    if name.chars().any(char::is_control) {
        return HttpHeaderNameShape::ContainsControl;
    }
    if name.chars().any(char::is_whitespace) {
        return HttpHeaderNameShape::ContainsWhitespace;
    }
    if name.contains(':') {
        return HttpHeaderNameShape::ContainsColon;
    }
    if !name.is_ascii() {
        return HttpHeaderNameShape::NonAscii;
    }
    HttpHeaderNameShape::Token
}

/// Return the stable structural shape for a header value without exposing it.
pub fn header_value_shape(value: &str) -> HttpHeaderValueShape {
    if value.contains('\0') {
        return HttpHeaderValueShape::ContainsNul;
    }
    if value.contains('\n') || value.contains('\r') {
        return HttpHeaderValueShape::ContainsLineBreak;
    }
    if value.is_empty() {
        return HttpHeaderValueShape::Empty;
    }
    HttpHeaderValueShape::Present
}

pub fn validate_header_name_shape(shape: HttpHeaderNameShape) -> Result<(), HttpHeaderShapeError> {
    if shape.is_allowed() {
        Ok(())
    } else {
        Err(HttpHeaderShapeError {
            field: "name",
            actual: shape.label(),
            expected: "ASCII header token without colon, whitespace, or controls",
        })
    }
}

pub fn validate_header_value_shape(
    shape: HttpHeaderValueShape,
) -> Result<(), HttpHeaderShapeError> {
    if shape.is_allowed() {
        Ok(())
    } else {
        Err(HttpHeaderShapeError {
            field: "value",
            actual: shape.label(),
            expected: "header value without NUL or line breaks",
        })
    }
}

/// Return the stable structural shape for a request URL without exposing it.
pub fn url_shape(url: &str) -> HttpUrlShape {
    if url.is_empty() {
        return HttpUrlShape::Empty;
    }

    let Some((scheme, rest)) = url.split_once("://") else {
        return HttpUrlShape::MissingScheme;
    };

    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return HttpUrlShape::UnsupportedScheme;
    }

    let authority = rest
        .split(|ch| matches!(ch, '/' | '?' | '#'))
        .next()
        .filter(|authority| !authority.is_empty());

    let Some(authority) = authority else {
        return HttpUrlShape::MissingHost;
    };

    if authority.contains('@') {
        return HttpUrlShape::ContainsCredentials;
    }

    if scheme == "https" {
        HttpUrlShape::Https
    } else {
        HttpUrlShape::Http
    }
}

/// Return the stable structural shape for a method without exposing custom text.
pub fn method_shape(method: &HttpMethod) -> HttpMethodShape {
    match method {
        HttpMethod::Get
        | HttpMethod::Post
        | HttpMethod::Put
        | HttpMethod::Patch
        | HttpMethod::Delete
        | HttpMethod::Head
        | HttpMethod::Options => HttpMethodShape::Standard,
        HttpMethod::Custom(raw) if raw.is_empty() => HttpMethodShape::Empty,
        HttpMethod::Custom(raw) if raw.chars().any(char::is_control) => {
            HttpMethodShape::ContainsControl
        }
        HttpMethod::Custom(raw) if raw.chars().any(char::is_whitespace) => {
            HttpMethodShape::ContainsWhitespace
        }
        HttpMethod::Custom(_) => HttpMethodShape::CustomToken,
    }
}

fn body_shape(request: &HttpRequest) -> HttpBodyShape {
    match &request.body {
        Some(body) if !body.is_empty() => HttpBodyShape::Present,
        _ => HttpBodyShape::Empty,
    }
}

fn method_diagnostic_label(method: &HttpMethod) -> &'static str {
    match method {
        HttpMethod::Get => "GET",
        HttpMethod::Post => "POST",
        HttpMethod::Put => "PUT",
        HttpMethod::Patch => "PATCH",
        HttpMethod::Delete => "DELETE",
        HttpMethod::Head => "HEAD",
        HttpMethod::Options => "OPTIONS",
        HttpMethod::Custom(_) => "CUSTOM",
    }
}

/// Validate an already-classified URL shape.
pub fn validate_url_shape(shape: HttpUrlShape) -> Result<(), HttpRequestShapeError> {
    if shape.is_allowed() {
        Ok(())
    } else {
        Err(HttpRequestShapeError {
            field: "url",
            actual: shape.label(),
            expected: "http|https URL with host and without embedded credentials",
        })
    }
}

/// Validate an already-classified HTTP method shape.
pub fn validate_method_shape(shape: HttpMethodShape) -> Result<(), HttpRequestShapeError> {
    if shape.is_allowed() {
        Ok(())
    } else {
        Err(HttpRequestShapeError {
            field: "method",
            actual: shape.label(),
            expected: "standard method or custom token without whitespace/control characters",
        })
    }
}

/// Validate a request against std.http boundary metadata.
pub fn validate_request_contract(request: &HttpRequest) -> Result<(), HttpRequestShapeError> {
    HttpRequestDescriptor::new(request).validate_request_shape()
}

// ── HTTP issue diagnostics ───────────────────────────────────────────────

/// Stable HTTP issue kinds for host and tooling diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum HttpIssueKind {
    PermissionDenied,
    Timeout,
    ConnectionFailed,
    InvalidRequest,
    ServerError,
    Other,
    UnsafeRequestShape,
    UnsafeHeaderShape,
}

impl HttpIssueKind {
    pub const fn code(self) -> &'static str {
        match self {
            Self::PermissionDenied => "std.http.permission_denied",
            Self::Timeout => "std.http.timeout",
            Self::ConnectionFailed => "std.http.connection_failed",
            Self::InvalidRequest => "std.http.invalid_request",
            Self::ServerError => "std.http.server_error",
            Self::Other => "std.http.other",
            Self::UnsafeRequestShape => "std.http.request.unsafe_shape",
            Self::UnsafeHeaderShape => "std.http.header.unsafe_shape",
        }
    }

    pub const fn category(self) -> &'static str {
        match self {
            Self::PermissionDenied => "capability",
            Self::Timeout => "timeout",
            Self::ConnectionFailed => "network",
            Self::InvalidRequest | Self::UnsafeRequestShape | Self::UnsafeHeaderShape => "shape",
            Self::ServerError => "remote-status",
            Self::Other => "host-error",
        }
    }
}

/// Redacted, machine-readable HTTP issue descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpIssue {
    pub kind: HttpIssueKind,
    pub code: &'static str,
    pub category: &'static str,
    pub method_shape: Option<HttpMethodShape>,
    pub url_shape: Option<HttpUrlShape>,
    pub header_name_shape: Option<HttpHeaderNameShape>,
    pub header_value_shape: Option<HttpHeaderValueShape>,
    pub status_class: Option<&'static str>,
    pub redacted: bool,
}

impl HttpIssue {
    fn new(kind: HttpIssueKind) -> Self {
        Self {
            kind,
            code: kind.code(),
            category: kind.category(),
            method_shape: None,
            url_shape: None,
            header_name_shape: None,
            header_value_shape: None,
            status_class: None,
            redacted: true,
        }
    }

    pub fn diagnostic_key(&self) -> String {
        let method = self
            .method_shape
            .map(HttpMethodShape::label)
            .unwrap_or("none");
        let url = self.url_shape.map(HttpUrlShape::label).unwrap_or("none");
        let header_name = self
            .header_name_shape
            .map(HttpHeaderNameShape::label)
            .unwrap_or("none");
        let header_value = self
            .header_value_shape
            .map(HttpHeaderValueShape::label)
            .unwrap_or("none");
        let status = self.status_class.unwrap_or("none");
        format!(
            "std.http.issue:{}:{}:{method}:{url}:{header_name}:{header_value}:{status}",
            self.category, self.code
        )
    }
}

/// Convert an HTTP host error into a stable redacted issue descriptor.
pub fn http_error_issue(error: &HttpError) -> HttpIssue {
    match error {
        HttpError::PermissionDenied => HttpIssue::new(HttpIssueKind::PermissionDenied),
        HttpError::Timeout => HttpIssue::new(HttpIssueKind::Timeout),
        HttpError::ConnectionFailed(_) => HttpIssue::new(HttpIssueKind::ConnectionFailed),
        HttpError::InvalidRequest(_) => HttpIssue::new(HttpIssueKind::InvalidRequest),
        HttpError::ServerError(status) => {
            let mut issue = HttpIssue::new(HttpIssueKind::ServerError);
            issue.status_class = Some(status_class(*status));
            issue
        }
        HttpError::Other(_) => HttpIssue::new(HttpIssueKind::Other),
    }
}

/// Convert request-shape failures into redacted diagnostics.
pub fn request_shape_issue(request: &HttpRequest, _error: &HttpRequestShapeError) -> HttpIssue {
    let descriptor = HttpRequestDescriptor::new(request);
    let mut issue = HttpIssue::new(HttpIssueKind::UnsafeRequestShape);
    issue.method_shape = Some(descriptor.method_shape);
    issue.url_shape = Some(descriptor.url_shape);
    issue
}

/// Convert header-shape failures into redacted diagnostics.
pub fn header_shape_issue(name: &str, value: &str, _error: &HttpHeaderShapeError) -> HttpIssue {
    let descriptor = HttpHeaderDescriptor::new(name, value);
    let mut issue = HttpIssue::new(HttpIssueKind::UnsafeHeaderShape);
    issue.header_name_shape = Some(descriptor.name_shape);
    issue.header_value_shape = Some(descriptor.value_shape);
    issue
}

fn status_class(status: StatusCode) -> &'static str {
    match status.0 {
        100..=199 => "1xx",
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        500..=599 => "5xx",
        _ => "non-standard",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_capability_labels_are_stable() {
        assert_eq!(HttpCapability::Call.label(), "http.call");
    }

    #[test]
    fn request_descriptors_make_explicit_capability_contract_visible() {
        let request = HttpRequest::post("https://api.example.test/v1/events", b"{}".to_vec());
        let descriptor = HttpRequestDescriptor::new(&request);

        assert_eq!(descriptor.capability, HttpCapability::Call);
        assert_eq!(descriptor.capability_label, "http.call");
        assert_eq!(descriptor.method_label, "POST");
        assert_eq!(descriptor.method_shape, HttpMethodShape::Standard);
        assert_eq!(descriptor.url_shape, HttpUrlShape::Https);
        assert_eq!(descriptor.body_shape, HttpBodyShape::Present);
        assert!(descriptor.grant_required);
        assert!(!descriptor.ambient_access);
        assert!(descriptor.timeout_required);
        assert!(descriptor.retry_policy_explicit);
        assert_eq!(
            descriptor.diagnostic_key(),
            "std.http.request:http.call:https:POST:body"
        );
        assert_eq!(descriptor.validate_request_shape(), Ok(()));
    }

    #[test]
    fn request_shape_validation_rejects_unsafe_urls_without_leaking_url() {
        let credentialed = HttpRequest::get("https://user:token@example.test/private");
        let descriptor = HttpRequestDescriptor::new(&credentialed);

        assert_eq!(descriptor.url_shape, HttpUrlShape::ContainsCredentials);
        assert_eq!(
            descriptor.diagnostic_key(),
            "std.http.request:http.call:contains-credentials:GET:empty-body"
        );
        assert_eq!(
            descriptor.validate_request_shape(),
            Err(HttpRequestShapeError {
                field: "url",
                actual: "contains-credentials",
                expected: "http|https URL with host and without embedded credentials",
            })
        );

        assert_eq!(url_shape(""), HttpUrlShape::Empty);
        assert_eq!(url_shape("example.test/path"), HttpUrlShape::MissingScheme);
        assert_eq!(
            url_shape("ftp://example.test/file"),
            HttpUrlShape::UnsupportedScheme
        );
        assert_eq!(
            url_shape("https:///missing-host"),
            HttpUrlShape::MissingHost
        );
    }

    #[test]
    fn request_shape_validation_rejects_invalid_custom_methods_without_leaking_text() {
        let mut request = HttpRequest::get("https://api.example.test");
        request.method = HttpMethod::Custom("BAD METHOD".to_string());
        let descriptor = HttpRequestDescriptor::new(&request);

        assert_eq!(descriptor.method_label, "CUSTOM");
        assert_eq!(descriptor.method_shape, HttpMethodShape::ContainsWhitespace);
        assert_eq!(
            descriptor.diagnostic_key(),
            "std.http.request:http.call:https:CUSTOM:empty-body"
        );
        assert_eq!(
            descriptor.validate_request_shape(),
            Err(HttpRequestShapeError {
                field: "method",
                actual: "contains-whitespace",
                expected: "standard method or custom token without whitespace/control characters",
            })
        );

        request.method = HttpMethod::Custom("PATCHX".to_string());
        assert_eq!(validate_request_contract(&request), Ok(()));
    }

    #[test]
    fn header_contracts_reject_injection_without_leaking_values() {
        let descriptor = HttpHeaderDescriptor::new(
            "Authorization",
            "Bearer secret
Injected: yes",
        );

        assert_eq!(descriptor.name_shape, HttpHeaderNameShape::Token);
        assert_eq!(
            descriptor.value_shape,
            HttpHeaderValueShape::ContainsLineBreak
        );
        assert!(descriptor.canonical_name_required);
        assert_eq!(
            descriptor.diagnostic_key(),
            "std.http.header:token:contains-line-break"
        );
        assert_eq!(
            descriptor.validate(),
            Err(HttpHeaderShapeError {
                field: "value",
                actual: "contains-line-break",
                expected: "header value without NUL or line breaks",
            })
        );
    }

    #[test]
    fn checked_header_insert_canonicalizes_safe_names() {
        let mut headers = HeaderMap::new();

        assert_eq!(headers.insert_checked("X-Trace-Id", "abc123"), Ok(()));
        assert_eq!(headers.get("x-trace-id"), Some("abc123"));
        assert_eq!(headers.get("X-TRACE-ID"), Some("abc123"));

        assert_eq!(
            headers.insert_checked("Bad Header", "ok"),
            Err(HttpHeaderShapeError {
                field: "name",
                actual: "contains-whitespace",
                expected: "ASCII header token without colon, whitespace, or controls",
            })
        );
        assert_eq!(
            headers.insert_checked("X-Secret", "token bytes"),
            Err(HttpHeaderShapeError {
                field: "value",
                actual: "contains-nul",
                expected: "header value without NUL or line breaks",
            })
        );
    }

    #[test]
    fn http_error_issue_redacts_host_messages() {
        let err = HttpError::ConnectionFailed("token=secret host unreachable".to_string());
        let issue = http_error_issue(&err);

        assert_eq!(issue.code, "std.http.connection_failed");
        assert_eq!(issue.category, "network");
        assert!(issue.redacted);
        assert_eq!(
            issue.diagnostic_key(),
            "std.http.issue:network:std.http.connection_failed:none:none:none:none:none"
        );
        assert!(!issue.diagnostic_key().contains("secret"));
    }

    #[test]
    fn request_shape_issue_uses_url_and_method_shapes() {
        let mut request = HttpRequest::get("https://user:secret@example.test/private");
        request.method = HttpMethod::Custom("BAD METHOD".to_string());
        let err = HttpRequestShapeError {
            field: "method",
            actual: "contains-whitespace",
            expected: "standard method or custom token without whitespace/control characters",
        };

        let issue = request_shape_issue(&request, &err);

        assert_eq!(issue.code, "std.http.request.unsafe_shape");
        assert_eq!(issue.category, "shape");
        assert_eq!(
            issue.method_shape,
            Some(HttpMethodShape::ContainsWhitespace)
        );
        assert_eq!(issue.url_shape, Some(HttpUrlShape::ContainsCredentials));
        assert_eq!(
            issue.diagnostic_key(),
            "std.http.issue:shape:std.http.request.unsafe_shape:contains-whitespace:contains-credentials:none:none:none"
        );
        assert!(!issue.diagnostic_key().contains("secret"));
    }

    #[test]
    fn header_shape_issue_redacts_header_values() {
        let err = HttpHeaderShapeError {
            field: "value",
            actual: "contains-line-break",
            expected: "header value without NUL or line breaks",
        };

        let issue = header_shape_issue("Authorization", "Bearer secret\r\nInjected: yes", &err);

        assert_eq!(issue.code, "std.http.header.unsafe_shape");
        assert_eq!(issue.category, "shape");
        assert_eq!(issue.header_name_shape, Some(HttpHeaderNameShape::Token));
        assert_eq!(
            issue.header_value_shape,
            Some(HttpHeaderValueShape::ContainsLineBreak)
        );
        assert!(!issue.diagnostic_key().contains("secret"));
        assert!(!issue.diagnostic_key().contains("Authorization"));
    }

    #[test]
    fn server_error_issue_reports_status_class_only() {
        let issue = http_error_issue(&HttpError::ServerError(StatusCode(503)));

        assert_eq!(issue.code, "std.http.server_error");
        assert_eq!(issue.category, "remote-status");
        assert_eq!(issue.status_class, Some("5xx"));
        assert_eq!(
            issue.diagnostic_key(),
            "std.http.issue:remote-status:std.http.server_error:none:none:none:none:5xx"
        );
    }
}
