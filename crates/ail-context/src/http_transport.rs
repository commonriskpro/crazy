// ── ail-context::http_transport ───────────────────────────────────────────
//
// Minimal HTTP/1.1 JSON-RPC transport for ContextServer (loopback, no TLS).
//
// Accepts a `ContextRpcRequest` JSON body in an HTTP POST request and
// returns the `ContextRpcResponse` as the HTTP response body.  Designed
// for loopback use: IDE extensions, local tooling, and integration tests.
//
// # Protocol
//
// - Only HTTP/1.1 POST is accepted.  Any other method → 405.
// - The request body must be a JSON-encoded `ContextRpcRequest` envelope.
// - On success the response is HTTP 200 with a JSON `ContextRpcResponse` body.
// - Malformed JSON bodies return HTTP 200 with a JSON-RPC -32700 error body.
// - Bodies larger than `max_body_bytes` → HTTP 413 (body is NOT read).
// - Each connection carries exactly one request/response pair then closes.
//
// # Guards
//
// Conservative defaults protect the server from runaway clients:
// - [`HTTP_MAX_HEADER_BYTES`] — total request-line + header bytes allowed.
// - [`HTTP_MAX_BODY_BYTES`]   — maximum body length announced by Content-Length.
// - [`HTTP_READ_TIMEOUT`]     — per-connection read deadline.
// - [`HTTP_WRITE_TIMEOUT`]    — per-connection write deadline.
//
// Callers may override the body limit with [`HttpTransport::with_max_body_bytes`].
//
// # Threading model
//
// `serve_one` handles a single connection synchronously on the calling
// thread, using `futures::executor::block_on` to drive
// `ContextServer::handle_rpc`.  For concurrent handling, accept in a loop
// and spawn one OS thread per stream.  `serve` provides a sequential
// single-threaded accept loop for simpler setups.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use futures::executor::block_on;

use crate::server::{ContextRpcRequest, ContextRpcResponse, ContextServer};
use crate::source::ContextSource;

// ── Reentrancy guard for the synchronous serve path ───────────────────────
//
// `serve_one` drives each `handle_rpc` future with
// `futures::executor::block_on`, which creates a lightweight single-threaded
// executor for each request.  Nesting two `block_on` calls on the SAME OS
// thread is architecturally unintended and signals that `serve_one` is being
// called re-entrantly (e.g. from inside a ContextSource implementation that
// somehow calls back into the transport).
//
// The guard is a thread-local boolean.  `BlockOnGuard` sets it to `true` on
// construction and resets it to `false` in its `Drop` impl.  Using RAII
// ensures the flag is always cleared — even when `block_on` panics and the
// stack unwinds — so subsequent calls on the same thread do not see a stale
// `true` and hit the debug assertion spuriously.
thread_local! {
    static BLOCK_ON_ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// RAII guard that sets `BLOCK_ON_ACTIVE` to `true` on construction and
/// resets it to `false` on drop — even when the guarded code panics.
struct BlockOnGuard;

impl BlockOnGuard {
    fn new() -> Self {
        BLOCK_ON_ACTIVE.with(|g| g.set(true));
        Self
    }
}

impl Drop for BlockOnGuard {
    fn drop(&mut self) {
        BLOCK_ON_ACTIVE.with(|g| g.set(false));
    }
}

// ── Limits and timeouts ───────────────────────────────────────────────────

/// Maximum total bytes consumed while reading the HTTP request line and headers.
pub const HTTP_MAX_HEADER_BYTES: usize = 8 * 1024; // 8 KiB

/// Maximum bytes allowed for the HTTP request body.
///
/// Requests whose `Content-Length` header exceeds this value are rejected
/// with HTTP 413 before any body bytes are read from the socket.
pub const HTTP_MAX_BODY_BYTES: usize = 512 * 1024; // 512 KiB

/// Per-connection read timeout applied to each accepted TCP stream.
pub const HTTP_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Per-connection write timeout applied to each accepted TCP stream.
pub const HTTP_WRITE_TIMEOUT: Duration = Duration::from_secs(30);

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
}

impl std::fmt::Display for HttpTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpTransportError::Accept(e) => write!(f, "HTTP accept error: {e}"),
            HttpTransportError::Read(e) => write!(f, "HTTP read error: {e}"),
            HttpTransportError::Write(e) => write!(f, "HTTP write error: {e}"),
            HttpTransportError::Encode(msg) => write!(f, "HTTP encode error: {msg}"),
        }
    }
}

impl std::error::Error for HttpTransportError {}

// ── HttpTransport ─────────────────────────────────────────────────────────

/// Minimal HTTP/1.1 JSON-RPC transport over a TCP listener.
///
/// Wraps a [`ContextServer`] and serves `ContextRpcRequest` / `ContextRpcResponse`
/// JSON-RPC envelopes as HTTP POST requests/responses.  Each accepted TCP
/// connection carries exactly one request/response pair; the connection is
/// closed afterwards.
///
/// # Example
///
/// ```rust,ignore
/// use std::net::TcpListener;
/// use ail_context::{ContextServer, HttpTransport};
/// use ail_context::source::InMemoryContextSource;
///
/// let server = ContextServer::new(InMemoryContextSource::new());
/// let transport = HttpTransport::new(server);
/// let listener = TcpListener::bind("127.0.0.1:0").unwrap();
/// transport.serve(listener).unwrap(); // loops until accept fails
/// ```
pub struct HttpTransport<S> {
    server: ContextServer<S>,
    max_header_bytes: usize,
    max_body_bytes: usize,
    read_timeout: Duration,
    write_timeout: Duration,
}

impl<S> HttpTransport<S> {
    /// Create a transport wrapping the given server with conservative default limits.
    pub fn new(server: ContextServer<S>) -> Self {
        Self {
            server,
            max_header_bytes: HTTP_MAX_HEADER_BYTES,
            max_body_bytes: HTTP_MAX_BODY_BYTES,
            read_timeout: HTTP_READ_TIMEOUT,
            write_timeout: HTTP_WRITE_TIMEOUT,
        }
    }

    /// Return a reference to the inner server.
    pub fn server(&self) -> &ContextServer<S> {
        &self.server
    }

    /// Override the maximum request body size in bytes (default: [`HTTP_MAX_BODY_BYTES`]).
    ///
    /// Requests whose `Content-Length` header exceeds this limit are rejected
    /// with HTTP 413 without reading any body bytes.
    pub fn with_max_body_bytes(mut self, limit: usize) -> Self {
        self.max_body_bytes = limit;
        self
    }
}

impl<S> HttpTransport<S>
where
    S: ContextSource + Send + Sync,
{
    /// Handle one accepted TCP connection synchronously.
    ///
    /// Sets read/write timeouts, parses the HTTP request, dispatches it
    /// through the context server, and writes the HTTP response.  The
    /// connection is closed when this method returns.
    ///
    /// Uses `futures::executor::block_on` internally to drive the async
    /// `handle_rpc` future.  Call from a plain OS thread or a
    /// `tokio::task::spawn_blocking` closure — not directly from inside
    /// an async executor task, which would block the executor thread.
    pub fn serve_one(&self, stream: TcpStream) -> Result<(), HttpTransportError> {
        stream
            .set_read_timeout(Some(self.read_timeout))
            .map_err(HttpTransportError::Read)?;
        stream
            .set_write_timeout(Some(self.write_timeout))
            .map_err(HttpTransportError::Write)?;

        // Clone the file descriptor so BufReader can own the read side while
        // we retain an independent write handle for the response.
        let mut writer = stream.try_clone().map_err(HttpTransportError::Write)?;
        let mut reader = BufReader::new(stream);

        let (method, content_length) = self.read_headers(&mut reader)?;

        if method != "POST" {
            write_http_response(
                &mut writer,
                405,
                "Method Not Allowed",
                b"Only POST is supported",
            )
            .map_err(HttpTransportError::Write)?;
            return Ok(());
        }

        if content_length > self.max_body_bytes {
            write_http_response(
                &mut writer,
                413,
                "Content Too Large",
                b"Request body exceeds limit",
            )
            .map_err(HttpTransportError::Write)?;
            return Ok(());
        }

        let mut body = vec![0u8; content_length];
        reader
            .read_exact(&mut body)
            .map_err(HttpTransportError::Read)?;

        debug_assert!(
            !BLOCK_ON_ACTIVE.with(|g| g.get()),
            "HttpTransport::serve_one is not re-entrant on the same OS thread; \
             if you are inside an async executor use a spawn_blocking wrapper instead"
        );
        let _guard = BlockOnGuard::new();
        let rpc_response = match serde_json::from_slice::<ContextRpcRequest>(&body) {
            Ok(request) => block_on(self.server.handle_rpc(request)),
            Err(e) => ContextRpcResponse::parse_error(e.to_string()),
        };

        let response_bytes = serde_json::to_vec(&rpc_response)
            .map_err(|e| HttpTransportError::Encode(e.to_string()))?;

        write_http_response(&mut writer, 200, "OK", &response_bytes)
            .map_err(HttpTransportError::Write)?;

        Ok(())
    }

    /// Serve all incoming connections on `listener` until `accept` fails.
    ///
    /// Each connection is dispatched to [`serve_one`][Self::serve_one] on the
    /// calling thread.  Per-connection errors are swallowed so the loop
    /// continues serving subsequent connections.  Returns only when
    /// `listener.accept()` itself returns an error.
    ///
    /// For concurrent request handling, accept connections in your own loop
    /// and spawn one OS thread per stream, delegating each to `serve_one`.
    pub fn serve(&self, listener: TcpListener) -> Result<(), HttpTransportError> {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    // Best-effort: swallow per-connection errors to keep the loop alive.
                    let _ = self.serve_one(stream);
                }
                Err(e) => return Err(HttpTransportError::Accept(e)),
            }
        }
        Ok(())
    }

    /// Read the HTTP request line and all header fields from `reader`.
    ///
    /// Returns `(method, content_length)`.  Stops at the empty line that
    /// separates headers from the body.  Returns an error if the cumulative
    /// bytes read exceed `max_header_bytes`.
    fn read_headers<R: BufRead>(
        &self,
        reader: &mut R,
    ) -> Result<(String, usize), HttpTransportError> {
        let mut header_bytes_read: usize = 0;
        let mut method = String::new();
        let mut content_length: usize = 0;
        let mut first_line = true;

        loop {
            let mut line = String::new();
            let n = reader
                .read_line(&mut line)
                .map_err(HttpTransportError::Read)?;
            if n == 0 {
                // EOF before the header terminator — treat as end of headers.
                break;
            }
            header_bytes_read = header_bytes_read.saturating_add(n);
            if header_bytes_read > self.max_header_bytes {
                return Err(HttpTransportError::Read(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "HTTP request headers exceed size limit",
                )));
            }

            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                break; // empty line marks the end of headers
            }

            if first_line {
                // Request line: METHOD SP request-target SP HTTP-version
                method = line.split_whitespace().next().unwrap_or("").to_string();
                first_line = false;
            } else {
                // Header field — case-insensitive match for Content-Length.
                let lower = trimmed.to_ascii_lowercase();
                if let Some(rest) = lower.strip_prefix("content-length:") {
                    content_length = rest.trim().parse().unwrap_or(0);
                }
            }
        }

        Ok((method, content_length))
    }
}

// ── HTTP framing helper ───────────────────────────────────────────────────

/// Write a minimal HTTP/1.1 response and flush the writer.
///
/// Always emits `Content-Type: application/json` and `Connection: close`.
/// For non-200 responses the body is plain text, but a uniform content-type
/// header simplifies client parsing.
fn write_http_response<W: Write>(
    writer: &mut W,
    status: u16,
    reason: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let body_len = body.len();
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {body_len}\r\n\
         Connection: close\r\n\
         \r\n"
    );
    writer.write_all(header.as_bytes())?;
    writer.write_all(body)?;
    writer.flush()
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::thread;
    use std::time::Duration;

    use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
    use ail_storage::graph::SnapshotEnvelope;
    use ail_storage::object::ObjectId;

    use super::*;
    use crate::dto::{ContextQuery, SnapshotSelector};
    use crate::server::{
        CONTEXT_RPC_QUERY_METHOD, ContextRequest, ContextResponse, JSONRPC_PARSE_ERROR,
    };
    use crate::source::InMemoryContextSource;
    use crate::{ContextRpcRequest, ContextRpcResponse, QueryBudget, QueryScope};

    // ── Test helpers ──────────────────────────────────────────────────────

    fn snapshot() -> SnapshotEnvelope {
        let id = ObjectId::from_bytes(b"http-snapshot");
        SnapshotEnvelope {
            id,
            graph_root_hash: id,
            parent_id: None,
            applied_change_id: None,
            created_at: 1,
            verification_report_hash: None,
            ..Default::default()
        }
    }

    fn graph() -> SemanticGraph {
        let node = GraphNode::new(NodeRef(0), NodeKind::Function, "http_fn");
        SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        }
    }

    fn make_transport() -> HttpTransport<InMemoryContextSource> {
        let snap = snapshot();
        let g = graph();
        let source = InMemoryContextSource::new();
        source.insert_snapshot(snap.clone());
        source.insert_graph(snap.graph_root_hash, g);
        HttpTransport::new(ContextServer::new(source))
    }

    fn valid_query_request(id: &str) -> ContextRpcRequest {
        let snap = snapshot();
        ContextRpcRequest::new(
            id,
            CONTEXT_RPC_QUERY_METHOD,
            ContextRequest::Query {
                query: ContextQuery::Graph {
                    scope: QueryScope::Full,
                    budget: QueryBudget::default(),
                },
                snapshot: SnapshotSelector::ById(snap.id),
                session: None,
            },
        )
    }

    /// Send an HTTP POST to `addr` with the given body and return the full
    /// raw HTTP response bytes (status line + headers + body).
    fn post(addr: SocketAddr, body: &[u8]) -> Vec<u8> {
        let mut stream = TcpStream::connect(addr).expect("connect");
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        let headers = format!(
            "POST / HTTP/1.1\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n",
            body.len()
        );
        stream.write_all(headers.as_bytes()).expect("write headers");
        stream.write_all(body).expect("write body");
        stream.flush().expect("flush");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).expect("read response");
        response
    }

    /// Parse the HTTP status code from a raw response.
    fn http_status(response: &[u8]) -> u16 {
        // Status line: "HTTP/1.1 200 OK\r\n..."
        let line = response
            .split(|&b| b == b'\n')
            .next()
            .and_then(|l| std::str::from_utf8(l).ok())
            .unwrap_or("");
        line.split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }

    /// Extract the HTTP response body (bytes after `\r\n\r\n`).
    fn extract_body(response: &[u8]) -> &[u8] {
        let sep = b"\r\n\r\n";
        response
            .windows(sep.len())
            .position(|w| w == sep)
            .map(|pos| &response[pos + sep.len()..])
            .unwrap_or(&[])
    }

    // ── loopback_query_returns_result ─────────────────────────────────────
    // Spec: a valid context.query POST over loopback returns HTTP 200 with
    //       a JSON-RPC Result response envelope.
    //
    // RED: HttpTransport did not exist.
    // GREEN: serve_one dispatches through handle_rpc and emits HTTP 200.
    #[test]
    fn loopback_query_returns_result() {
        let transport = make_transport();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let request = valid_query_request("h-1");
        let body = serde_json::to_vec(&request).unwrap();

        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            transport.serve_one(stream).expect("serve_one");
        });

        let response = post(addr, &body);
        handle.join().expect("thread join");

        assert_eq!(http_status(&response), 200);
        let rpc: ContextRpcResponse =
            serde_json::from_slice(extract_body(&response)).expect("parse JSON-RPC response");
        assert_eq!(rpc.id, "h-1");
        assert!(rpc.error.is_none(), "unexpected RPC error: {:?}", rpc.error);
        assert!(
            matches!(rpc.result, Some(ContextResponse::Result(_))),
            "expected Result variant, got: {:?}",
            rpc.result
        );
    }

    // ── malformed_json_body_returns_parse_error ───────────────────────────
    // Spec: a POST with a malformed JSON body returns HTTP 200 with a
    //       JSON-RPC -32700 parse-error response.
    //
    // RED: HttpTransport did not exist.
    // GREEN: serve_one detects serde_json error and calls parse_error.
    #[test]
    fn malformed_json_body_returns_parse_error() {
        let transport = make_transport();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            transport.serve_one(stream).expect("serve_one");
        });

        let response = post(addr, b"{not valid json}");
        handle.join().expect("thread join");

        assert_eq!(http_status(&response), 200);
        let rpc: ContextRpcResponse =
            serde_json::from_slice(extract_body(&response)).expect("parse JSON-RPC response");
        assert!(
            rpc.result.is_none(),
            "result must be absent for parse errors"
        );
        let err = rpc.error.expect("error must be set");
        assert_eq!(
            err.code, JSONRPC_PARSE_ERROR,
            "parse error must use code {JSONRPC_PARSE_ERROR}"
        );
    }

    // ── body_too_large_returns_413 ────────────────────────────────────────
    // Spec: a POST whose Content-Length exceeds max_body_bytes returns
    //       HTTP 413 without reading any body bytes.
    //
    // RED: HttpTransport did not exist.
    // GREEN: serve_one checks content_length before read_exact.
    #[test]
    fn body_too_large_returns_413() {
        let transport = make_transport().with_max_body_bytes(16);
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            transport.serve_one(stream).expect("serve_one");
        });

        // Send only the request headers, with a Content-Length that exceeds
        // the 16-byte limit.  The server must respond 413 without waiting
        // for the body bytes that are never sent.
        let mut stream = TcpStream::connect(addr).expect("connect");
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        stream
            .write_all(
                b"POST / HTTP/1.1\r\n\
                  Content-Length: 1024\r\n\
                  Connection: close\r\n\
                  \r\n",
            )
            .expect("write headers");
        stream.flush().expect("flush");

        let mut response = Vec::new();
        stream.read_to_end(&mut response).expect("read response");
        handle.join().expect("thread join");

        assert_eq!(
            http_status(&response),
            413,
            "expected HTTP 413, got: {}",
            String::from_utf8_lossy(&response)
        );
    }

    // ── non_post_method_returns_405 ───────────────────────────────────────
    // Spec: a GET (or any non-POST) request returns HTTP 405 Method Not Allowed.
    //
    // RED: HttpTransport did not exist.
    // GREEN: serve_one inspects the request line method and returns 405.
    #[test]
    fn non_post_method_returns_405() {
        let transport = make_transport();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            transport.serve_one(stream).expect("serve_one");
        });

        let mut stream = TcpStream::connect(addr).expect("connect");
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        stream
            .write_all(b"GET / HTTP/1.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .expect("write");
        stream.flush().expect("flush");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).expect("read");
        handle.join().expect("thread join");

        assert_eq!(
            http_status(&response),
            405,
            "expected HTTP 405, got: {}",
            String::from_utf8_lossy(&response)
        );
    }
}
