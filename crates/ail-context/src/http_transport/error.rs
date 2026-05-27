// ── HttpTransportError ────────────────────────────────────────────────────

/// Errors that can occur in the HTTP transport framing layer.
///
/// These are distinct from [`ContextError`][crate::error::ContextError]:
/// transport errors are I/O or framing failures, not semantic query failures.
/// Semantic failures are returned as JSON-RPC error responses inside the
/// normal HTTP 200 response envelope.
#[derive(Debug)]
pub enum HttpTransportError {
    /// Failed to accept a new TCP connection from the listener.
    Accept(std::io::Error),
    /// I/O failure reading request data from a connection.
    Read(std::io::Error),
    /// I/O failure writing response data to a connection.
    Write(std::io::Error),
    /// The JSON-RPC response could not be serialized to JSON.
    ///
    /// This should not occur in practice because all response types derive
    /// `Serialize`; it is retained as a safety net against future changes.
    Encode(String),
    /// The accepted connection's peer address is not a loopback address.
    ///
    /// Returned by [`HttpTransport::serve_one`] when `loopback_only` is
    /// enabled (the default for local/dev use) and the peer IP is not a
    /// loopback address (`127.0.0.0/8` or `::1`).  The connection is
    /// dropped without sending any response body — this prevents probing.
    NonLoopback(std::net::SocketAddr),
}

impl std::fmt::Display for HttpTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpTransportError::Accept(e) => write!(f, "HTTP accept error: {e}"),
            HttpTransportError::Read(e) => write!(f, "HTTP read error: {e}"),
            HttpTransportError::Write(e) => write!(f, "HTTP write error: {e}"),
            HttpTransportError::Encode(msg) => write!(f, "HTTP encode error: {msg}"),
            HttpTransportError::NonLoopback(addr) => {
                write!(f, "HTTP connection rejected: non-loopback peer {addr}")
            }
        }
    }
}

impl std::error::Error for HttpTransportError {}
