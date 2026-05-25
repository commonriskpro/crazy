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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

// ── Query audit ───────────────────────────────────────────────────────────

/// Transport-level outcome for a single HTTP/JSON-RPC connection.
///
/// Recorded in a [`QueryAuditEvent`] and stored in the [`QueryAuditLog`].
/// Does **not** contain query body content, snapshot data, or secrets — only
/// the framing-level classification needed for local observability.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuditOutcome {
    /// The JSON-RPC request parsed successfully and the server returned a
    /// result response (no error code).
    Success,
    /// The HTTP body could not be deserialized as a valid [`ContextRpcRequest`].
    ParseError,
    /// The request parsed and dispatched, but the server returned a
    /// JSON-RPC error response.  `code` is the JSON-RPC error code.
    RpcError {
        /// JSON-RPC error code (e.g. `-32700`, `-32601`).
        code: i64,
    },
    /// The connection was dropped because the peer address is not loopback.
    /// Only occurs when `loopback_only = true` (the default).
    NonLoopback,
    /// The request used a non-POST HTTP method (e.g. GET); HTTP 405 returned.
    MethodNotAllowed,
    /// The declared `Content-Length` exceeded the configured body limit;
    /// HTTP 413 returned without reading any body bytes.
    BodyTooLarge,
    /// An I/O, timeout, or serialization error prevented normal completion.
    TransportError,
}

/// Audit record for a single HTTP connection accepted by [`HttpTransport`].
///
/// Fields are chosen to be observability-useful but **never contain raw query
/// content, snapshot bytes, or secret values**.  The struct is safe to log
/// or export without further redaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryAuditEvent {
    /// Monotonically increasing sequence number within this
    /// [`HttpTransport`] instance (1-based; wraps on `u64` overflow).
    pub seq: u64,
    /// Peer socket address.  Always a loopback address in the default
    /// `loopback_only = true` configuration.
    pub peer: std::net::SocketAddr,
    /// JSON-RPC method name (e.g. `"context.query"`), or `None` when the
    /// request body could not be parsed.  Never contains request body content.
    pub method: Option<String>,
    /// How the connection was resolved.
    pub outcome: AuditOutcome,
    /// Wall-clock milliseconds from connection acceptance to response flush.
    pub elapsed_ms: u64,
}

/// Shared, append-only audit log for all connections handled by an
/// [`HttpTransport`] instance.
///
/// Cloning the handle gives a second view onto the same underlying list;
/// useful for inspecting the log from a test thread while the transport
/// serves on another thread.
///
/// # Example
///
/// ```rust,ignore
/// let log = transport.audit_log().clone();
/// std::thread::spawn(move || transport.serve(listener).unwrap());
/// // after serving:
/// let events = log.snapshot();
/// assert_eq!(events[0].outcome, AuditOutcome::Success);
/// ```
#[derive(Clone, Default)]
pub struct QueryAuditLog(Arc<Mutex<Vec<QueryAuditEvent>>>);

impl QueryAuditLog {
    /// Return a point-in-time copy of all recorded events in insertion order.
    ///
    /// **Insertion order vs. `seq` order**: under the sequential [`HttpTransport::serve`]
    /// loop each connection is handled one at a time, so insertion order and `seq` order
    /// are identical.  If [`HttpTransport::serve_one`] is called concurrently from
    /// multiple threads — for example in a custom accept loop that spawns one thread per
    /// connection — `seq` is assigned atomically *before* connection work begins but the
    /// `Vec` push (insertion) happens *after*.  A thread that received `seq=2` can
    /// therefore push its event before the thread with `seq=1` finishes.  In that case,
    /// sort the snapshot by `seq` to restore arrival order.
    pub fn snapshot(&self) -> Vec<QueryAuditEvent> {
        self.0.lock().unwrap().clone()
    }

    /// Number of events recorded so far.
    pub fn len(&self) -> usize {
        self.0.lock().unwrap().len()
    }

    /// Returns `true` when no events have been recorded yet.
    pub fn is_empty(&self) -> bool {
        self.0.lock().unwrap().is_empty()
    }

    fn record(&self, event: QueryAuditEvent) {
        self.0.lock().unwrap().push(event);
    }
}

// ── HttpTransport ─────────────────────────────────────────────────────────

/// Minimal HTTP/1.1 JSON-RPC transport over a TCP listener.
///
/// Wraps a [`ContextServer`] and serves `ContextRpcRequest` / `ContextRpcResponse`
/// JSON-RPC envelopes as HTTP POST requests/responses.  Each accepted TCP
/// connection carries exactly one request/response pair; the connection is
/// closed afterwards.
///
/// # Local / dev hardening
///
/// By default (`loopback_only = true`) every accepted connection is validated
/// against its peer IP address.  Connections from non-loopback peers are
/// silently dropped — no response is sent, which prevents probing.  This
/// guards against accidental public exposure when a caller binds the listener
/// to `0.0.0.0` instead of `127.0.0.1`.
///
/// Disable via [`with_loopback_only(false)`][Self::with_loopback_only] only
/// in integration tests or future remote-transport wrappers that enforce
/// their own access control.
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
    /// Reject connections whose peer IP is not a loopback address.
    loopback_only: bool,
    /// Append-only audit log; one entry per accepted connection.
    audit_log: QueryAuditLog,
    /// Monotonic connection counter; wraps on `u64` overflow (unreachable in practice).
    next_seq: AtomicU64,
}

impl<S> HttpTransport<S> {
    /// Create a transport wrapping the given server with conservative default limits.
    ///
    /// `loopback_only` defaults to `true`: connections from non-loopback peers
    /// are rejected without sending any response.  See
    /// [`with_loopback_only`][Self::with_loopback_only] to override.
    pub fn new(server: ContextServer<S>) -> Self {
        Self {
            server,
            max_header_bytes: HTTP_MAX_HEADER_BYTES,
            max_body_bytes: HTTP_MAX_BODY_BYTES,
            read_timeout: HTTP_READ_TIMEOUT,
            write_timeout: HTTP_WRITE_TIMEOUT,
            loopback_only: true,
            audit_log: QueryAuditLog::default(),
            next_seq: AtomicU64::new(0),
        }
    }

    /// Return a reference to the inner server.
    pub fn server(&self) -> &ContextServer<S> {
        &self.server
    }

    /// Return a reference to the shared audit log.
    ///
    /// Clone the returned handle to observe events from another thread while
    /// the transport is serving.  See [`QueryAuditLog`] for usage.
    pub fn audit_log(&self) -> &QueryAuditLog {
        &self.audit_log
    }

    /// Override the maximum request body size in bytes (default: [`HTTP_MAX_BODY_BYTES`]).
    ///
    /// Requests whose `Content-Length` header exceeds this limit are rejected
    /// with HTTP 413 without reading any body bytes.
    pub fn with_max_body_bytes(mut self, limit: usize) -> Self {
        self.max_body_bytes = limit;
        self
    }

    /// Override the loopback-only peer filter (default: `true`).
    ///
    /// When `true` (the default), [`serve_one`][Self::serve_one] rejects any
    /// connection whose peer IP is not a loopback address (`127.0.0.0/8` or
    /// `::1`) by returning
    /// [`Err(HttpTransportError::NonLoopback)`][HttpTransportError::NonLoopback]
    /// without sending any response body.  This guards against accidental
    /// public exposure when the listener is bound to `0.0.0.0`.
    ///
    /// Set to `false` only in integration tests or future remote-transport
    /// wrappers that enforce their own access control.
    pub fn with_loopback_only(mut self, enabled: bool) -> Self {
        self.loopback_only = enabled;
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
    /// Every connection — successful or not — appends a [`QueryAuditEvent`]
    /// to this transport's [`QueryAuditLog`].  The event records the peer
    /// address, JSON-RPC method name, and outcome classification but **never**
    /// raw query content or secret values.
    ///
    /// Uses `futures::executor::block_on` internally to drive the async
    /// `handle_rpc` future.  Call from a plain OS thread or a
    /// `tokio::task::spawn_blocking` closure — not directly from inside
    /// an async executor task, which would block the executor thread.
    pub fn serve_one(&self, stream: TcpStream) -> Result<(), HttpTransportError> {
        let start = Instant::now();
        // Seq is assigned atomically here, before any connection work.
        // In the sequential `serve` loop this means insertion order == seq order.
        // If `serve_one` is called concurrently (e.g. one OS thread per accepted
        // stream), the thread with seq=2 can push its audit event before the thread
        // with seq=1 completes — so insertion order may differ from seq order.
        // Callers using a concurrent accept loop should sort the snapshot by `seq`
        // rather than relying on slice position for ordering.
        let seq = self
            .next_seq
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);

        // Best-effort peer address for audit and loopback guard.
        // `peer_addr()` is a cheap syscall; an error here is only possible on
        // already-closed sockets (vanishingly rare), so we fall back to an
        // unspecified sentinel rather than failing the whole connection.
        let peer: std::net::SocketAddr = stream
            .peer_addr()
            .unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap());

        let mut rpc_method: Option<String> = None;
        let mut outcome = AuditOutcome::TransportError;

        let result = self.serve_connection(stream, peer, &mut rpc_method, &mut outcome);

        self.audit_log.record(QueryAuditEvent {
            seq,
            peer,
            method: rpc_method,
            outcome,
            elapsed_ms: start.elapsed().as_millis() as u64,
        });

        result
    }

    /// Inner implementation of [`serve_one`][Self::serve_one].
    ///
    /// Separated to let `serve_one` wrap it with clean audit-event recording
    /// at a single call site.  The `rpc_method` and `outcome` out-parameters
    /// are updated as the connection progresses so that even early returns
    /// produce accurate audit events.
    fn serve_connection(
        &self,
        stream: TcpStream,
        peer: std::net::SocketAddr,
        rpc_method: &mut Option<String>,
        outcome: &mut AuditOutcome,
    ) -> Result<(), HttpTransportError> {
        stream
            .set_read_timeout(Some(self.read_timeout))
            .map_err(HttpTransportError::Read)?;
        stream
            .set_write_timeout(Some(self.write_timeout))
            .map_err(HttpTransportError::Write)?;

        // ── Loopback-only peer guard ──────────────────────────────────────
        //
        // Validate the peer address before reading any request data.  If the
        // peer is not loopback, we drop the connection silently (no response
        // body) to prevent probing.  This guards against accidental public
        // exposure when the caller binds to 0.0.0.0 instead of 127.0.0.1.
        if self.loopback_only
            && let Err(e) = check_peer_addr(peer, self.loopback_only)
        {
            *outcome = AuditOutcome::NonLoopback;
            return Err(e);
        }

        // Clone the file descriptor so BufReader can own the read side while
        // we retain an independent write handle for the response.
        let mut writer = stream.try_clone().map_err(HttpTransportError::Write)?;
        let mut reader = BufReader::new(stream);

        let (method, content_length) = self.read_headers(&mut reader)?;

        if method != "POST" {
            *outcome = AuditOutcome::MethodNotAllowed;
            // If the write itself fails, reclassify to TransportError so the audit
            // event accurately distinguishes "client received 405" from "405 was
            // attempted but the write also failed".  Mirrors the 200 response path.
            if let Err(e) = write_http_response(
                &mut writer,
                405,
                "Method Not Allowed",
                b"Only POST is supported",
            ) {
                *outcome = AuditOutcome::TransportError;
                return Err(HttpTransportError::Write(e));
            }
            return Ok(());
        }

        if content_length > self.max_body_bytes {
            *outcome = AuditOutcome::BodyTooLarge;
            // Reclassify to TransportError if the write fails — same rationale as
            // the 405 path above: the audit event should reflect whether the error
            // response was actually delivered to the client.
            if let Err(e) = write_http_response(
                &mut writer,
                413,
                "Content Too Large",
                b"Request body exceeds limit",
            ) {
                *outcome = AuditOutcome::TransportError;
                return Err(HttpTransportError::Write(e));
            }
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
            Ok(request) => {
                *rpc_method = Some(request.method.clone());
                let resp = block_on(self.server.handle_rpc(request));
                // Classify outcome from the RPC response before writing it.
                *outcome = match &resp.error {
                    Some(err) => AuditOutcome::RpcError { code: err.code },
                    None => AuditOutcome::Success,
                };
                resp
            }
            Err(e) => {
                *outcome = AuditOutcome::ParseError;
                ContextRpcResponse::parse_error(e.to_string())
            }
        };

        let response_bytes = match serde_json::to_vec(&rpc_response) {
            Ok(bytes) => bytes,
            Err(e) => {
                *outcome = AuditOutcome::TransportError;
                return Err(HttpTransportError::Encode(e.to_string()));
            }
        };

        if let Err(e) = write_http_response(&mut writer, 200, "OK", &response_bytes) {
            *outcome = AuditOutcome::TransportError;
            return Err(HttpTransportError::Write(e));
        }

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

// ── Loopback peer address check ───────────────────────────────────────────

/// Returns `Ok(())` when `loopback_only` is `false` or the peer IP is a
/// loopback address; otherwise returns `Err(HttpTransportError::NonLoopback(addr))`.
///
/// Uses [`IpAddr::to_canonical`] before [`IpAddr::is_loopback`] so that
/// IPv4-mapped IPv6 loopback addresses (`::ffff:127.0.0.1`) are accepted on
/// dual-stack sockets alongside `127.0.0.1` and `::1`.
///
/// Extracted from [`HttpTransport::serve_one`] to allow exhaustive unit
/// testing of the IP classification logic without spinning up real TCP
/// connections.
fn check_peer_addr(
    addr: std::net::SocketAddr,
    loopback_only: bool,
) -> Result<(), HttpTransportError> {
    if loopback_only && !addr.ip().to_canonical().is_loopback() {
        Err(HttpTransportError::NonLoopback(addr))
    } else {
        Ok(())
    }
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

    // ── check_peer_addr unit tests ────────────────────────────────────────
    //
    // Spec: when loopback_only=true, non-loopback peers are rejected;
    //       loopback peers (IPv4 + IPv6) are accepted.
    //       When loopback_only=false, all peers are accepted.
    //
    // These tests exercise the classification logic without real TCP sockets
    // so they run fast and deterministically on any platform.

    // ── check_peer_addr_rejects_ipv4_non_loopback ─────────────────────────
    // Spec: loopback_only=true, peer=192.168.1.1 → Err(NonLoopback)
    //
    // RED: no check_peer_addr function.
    // GREEN: check_peer_addr returns Err when ip is not loopback.
    #[test]
    fn check_peer_addr_rejects_ipv4_non_loopback() {
        use std::net::SocketAddr;
        let addr: SocketAddr = "192.168.1.1:12345".parse().unwrap();
        assert!(
            matches!(
                check_peer_addr(addr, true),
                Err(HttpTransportError::NonLoopback(_))
            ),
            "192.168.1.1 must be rejected with loopback_only=true"
        );
    }

    // ── check_peer_addr_rejects_ipv6_non_loopback ─────────────────────────
    // Spec: loopback_only=true, peer=2001:db8::1 → Err(NonLoopback)
    //
    // RED: no check_peer_addr function.
    // GREEN: check_peer_addr returns Err when ip is not loopback.
    #[test]
    fn check_peer_addr_rejects_ipv6_non_loopback() {
        use std::net::SocketAddr;
        let addr: SocketAddr = "[2001:db8::1]:8080".parse().unwrap();
        assert!(
            matches!(
                check_peer_addr(addr, true),
                Err(HttpTransportError::NonLoopback(_))
            ),
            "2001:db8::1 must be rejected with loopback_only=true"
        );
    }

    // ── check_peer_addr_accepts_ipv4_mapped_ipv6_loopback ─────────────────
    // Spec: loopback_only=true, peer=::ffff:127.0.0.1 → Ok(())
    //
    // Dual-stack sockets on Linux/macOS deliver IPv4 clients as
    // IPv4-mapped IPv6 addresses (::ffff:127.x.x.x).  `is_loopback()` alone
    // returns false for these; `to_canonical().is_loopback()` is required.
    //
    // RED: check_peer_addr used is_loopback() directly → rejected mapped loopback.
    // GREEN: check_peer_addr uses to_canonical().is_loopback() → accepts it.
    #[test]
    fn check_peer_addr_accepts_ipv4_mapped_ipv6_loopback() {
        use std::net::SocketAddr;
        let addr: SocketAddr = "[::ffff:127.0.0.1]:12345".parse().unwrap();
        assert!(
            check_peer_addr(addr, true).is_ok(),
            "::ffff:127.0.0.1 must be accepted with loopback_only=true"
        );
    }

    // ── check_peer_addr_accepts_ipv4_loopback ─────────────────────────────
    // Spec: loopback_only=true, peer=127.0.0.1 → Ok(())
    //
    // RED: no check_peer_addr function.
    // GREEN: check_peer_addr returns Ok for 127.0.0.1.
    #[test]
    fn check_peer_addr_accepts_ipv4_loopback() {
        use std::net::SocketAddr;
        let addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        assert!(
            check_peer_addr(addr, true).is_ok(),
            "127.0.0.1 must be accepted with loopback_only=true"
        );
    }

    // ── check_peer_addr_accepts_ipv6_loopback ─────────────────────────────
    // Spec: loopback_only=true, peer=::1 → Ok(())
    //
    // RED: no check_peer_addr function.
    // GREEN: check_peer_addr returns Ok for ::1.
    #[test]
    fn check_peer_addr_accepts_ipv6_loopback() {
        use std::net::SocketAddr;
        let addr: SocketAddr = "[::1]:12345".parse().unwrap();
        assert!(
            check_peer_addr(addr, true).is_ok(),
            "::1 must be accepted with loopback_only=true"
        );
    }

    // ── check_peer_addr_disabled_accepts_non_loopback ─────────────────────
    // Spec: loopback_only=false, any peer → Ok(())
    //
    // RED: no check_peer_addr function.
    // GREEN: check_peer_addr bypasses the IP check when disabled.
    #[test]
    fn check_peer_addr_disabled_accepts_non_loopback() {
        use std::net::SocketAddr;
        let addr: SocketAddr = "10.0.0.1:9999".parse().unwrap();
        assert!(
            check_peer_addr(addr, false).is_ok(),
            "10.0.0.1 must be accepted when loopback_only=false"
        );
    }

    // ── loopback_only_transport_accepts_loopback_connection ───────────────
    // Spec: HttpTransport::new (loopback_only=true) accepts a real 127.0.0.1
    //       client and dispatches the query normally.
    //
    // RED: loopback_only field did not exist; serve_one never checked peer.
    // GREEN: serve_one validates peer addr; 127.0.0.1 passes and query succeeds.
    #[test]
    fn loopback_only_transport_accepts_loopback_connection() {
        let transport = make_transport(); // default: loopback_only=true
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let request = valid_query_request("h-lo-accept");
        let body = serde_json::to_vec(&request).unwrap();

        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            transport
                .serve_one(stream)
                .expect("serve_one must succeed for loopback client")
        });

        let response = post(addr, &body);
        handle.join().expect("thread join");

        assert_eq!(
            http_status(&response),
            200,
            "loopback client must receive HTTP 200 from loopback_only transport"
        );
    }

    // ── with_loopback_only_false_accepts_loopback_client ─────────────────
    // Spec: with_loopback_only(false) disables the guard; loopback clients
    //       still work normally (guard disabled does not break dispatch).
    //
    // RED: with_loopback_only builder did not exist.
    // GREEN: builder sets loopback_only=false; loopback client gets 200.
    #[test]
    fn with_loopback_only_false_accepts_loopback_client() {
        let transport = make_transport().with_loopback_only(false);
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let request = valid_query_request("h-nolo");
        let body = serde_json::to_vec(&request).unwrap();

        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            transport
                .serve_one(stream)
                .expect("serve_one must succeed when loopback_only=false")
        });

        let response = post(addr, &body);
        handle.join().expect("thread join");

        assert_eq!(
            http_status(&response),
            200,
            "loopback client must receive HTTP 200 when loopback_only=false"
        );
    }

    // ── Query audit log tests ─────────────────────────────────────────────

    // ── audit_log_records_successful_query_event ──────────────────────────
    // Spec: after serving one valid context.query POST, the audit log
    //       contains exactly one event with outcome=Success and the correct
    //       method name.
    //
    // RED: HttpTransport had no audit_log field.
    // GREEN: serve_one records the event via serve_connection out-params.
    #[test]
    fn audit_log_records_successful_query_event() {
        let transport = make_transport();
        let log = transport.audit_log().clone();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let request = valid_query_request("audit-1");
        let body = serde_json::to_vec(&request).unwrap();

        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            transport.serve_one(stream).expect("serve_one");
        });

        post(addr, &body);
        handle.join().expect("thread join");

        let events = log.snapshot();
        assert_eq!(events.len(), 1, "exactly one audit event must be recorded");
        let ev = &events[0];
        assert_eq!(ev.seq, 1, "first event must have seq=1");
        assert!(ev.peer.ip().is_loopback(), "peer must be loopback");
        assert_eq!(
            ev.method.as_deref(),
            Some(crate::server::CONTEXT_RPC_QUERY_METHOD),
            "method must be context.query"
        );
        assert_eq!(
            ev.outcome,
            AuditOutcome::Success,
            "valid query must produce Success outcome"
        );
    }

    // ── audit_log_records_parse_error_event ───────────────────────────────
    // Spec: after serving a POST with a malformed JSON body, the audit log
    //       contains one event with outcome=ParseError and method=None.
    //
    // RED: HttpTransport had no audit_log field.
    // GREEN: serve_connection sets *outcome = ParseError on serde_json failure.
    #[test]
    fn audit_log_records_parse_error_event() {
        let transport = make_transport();
        let log = transport.audit_log().clone();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            transport.serve_one(stream).expect("serve_one");
        });

        post(addr, b"{not valid json}");
        handle.join().expect("thread join");

        let events = log.snapshot();
        assert_eq!(events.len(), 1, "exactly one audit event must be recorded");
        let ev = &events[0];
        assert_eq!(
            ev.outcome,
            AuditOutcome::ParseError,
            "malformed body must produce ParseError"
        );
        assert!(ev.method.is_none(), "method must be None when parse fails");
    }

    // ── audit_log_sequential_seqs ─────────────────────────────────────────
    // Spec: two consecutive (non-concurrent) serve_one calls produce events
    //       with seq=1 then seq=2 in the same audit log, with insertion order
    //       matching seq order.
    //
    // Note: under a concurrent accept loop (multiple threads each calling
    // serve_one simultaneously), seq is assigned atomically before connection
    // work but the Vec push happens after — so a thread with seq=2 can insert
    // its event before the one with seq=1.  This test exercises the sequential
    // path only.  Concurrent callers should sort snapshot() by `seq` rather
    // than trusting slice position.
    //
    // RED: next_seq field did not exist.
    // GREEN: next_seq AtomicU64 provides monotonic per-instance numbering.
    #[test]
    fn audit_log_sequential_seqs() {
        let transport = make_transport();
        let log = transport.audit_log().clone();

        let request = valid_query_request("seq-test");
        let body = serde_json::to_vec(&request).unwrap();

        for _ in 0..2 {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
            let addr = listener.local_addr().expect("local_addr");
            // Serve on the main thread (transport is not Send) by accepting
            // after the client thread has sent the request.  Clone body each
            // iteration so the closure captures its own copy.
            let body_clone = body.clone();
            let client = thread::spawn(move || post(addr, &body_clone));
            let (stream, _) = listener.accept().expect("accept");
            transport.serve_one(stream).expect("serve_one");
            client.join().expect("client join");
        }

        let events = log.snapshot();
        assert_eq!(
            events.len(),
            2,
            "two serve_one calls must produce two events"
        );
        assert_eq!(events[0].seq, 1, "first event must have seq=1");
        assert_eq!(events[1].seq, 2, "second event must have seq=2");
    }

    // ── audit_event_fields_contain_no_secret ──────────────────────────────
    // Spec: even when the graph has a node carrying a sensitive literal value,
    //       the QueryAuditEvent Debug representation must not contain that
    //       value — proving that no query body content leaks into audit records.
    //
    // This is the transport-level analogue of the server-level redaction
    // guarantee test in tests/context_response.rs.
    //
    // RED: no audit log existed; no proof that transport-layer observability
    //      records are safe to emit without further redaction.
    // GREEN: QueryAuditEvent only records method name, peer addr, outcome, and
    //        elapsed_ms — none of which can carry query body content.
    #[test]
    fn audit_event_fields_contain_no_secret() {
        use crate::server::{ContextServer, ContextServerConfig, FieldRedactionRule, TrustLevel};
        use crate::source::InMemoryContextSource;
        use ail_core::semantic_graph::{NodeKind, NodeRef, SemanticGraph};
        use ail_storage::graph::SnapshotEnvelope;
        use ail_storage::object::ObjectId;

        const SECRET: &str = "TRANSPORT-AUDIT-SECRET-abc999";

        let snap_id = ObjectId::from_bytes(b"audit-secret-snap");
        let snap = SnapshotEnvelope {
            id: snap_id,
            graph_root_hash: snap_id,
            parent_id: None,
            applied_change_id: None,
            created_at: 1,
            verification_report_hash: None,
            ..Default::default()
        };

        let mut sensitive =
            ail_core::semantic_graph::GraphNode::new(NodeRef(0), NodeKind::Function, "secret_fn");
        sensitive.body_expr = Some(SECRET.to_string());
        let graph = SemanticGraph {
            nodes: vec![sensitive],
            edges: vec![],
        };

        let source = InMemoryContextSource::new();
        source.insert_snapshot(snap.clone());
        source.insert_graph(snap.graph_root_hash, graph);

        let server = ContextServer::new(source).with_config(ContextServerConfig {
            redaction_rules: vec![FieldRedactionRule {
                field: "body_expr".to_string(),
                min_trust: TrustLevel::Privileged,
                category: "secrets".to_string(),
            }],
            ..Default::default()
        });

        let transport = HttpTransport::new(server);
        let log = transport.audit_log().clone();
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");

        // Issue a graph query (no session = public trust; body_expr is redacted).
        let request = ContextRpcRequest::new(
            "audit-secret",
            crate::server::CONTEXT_RPC_QUERY_METHOD,
            crate::server::ContextRequest::Query {
                query: crate::dto::ContextQuery::Graph {
                    scope: crate::QueryScope::Full,
                    budget: crate::QueryBudget::default(),
                },
                snapshot: crate::dto::SnapshotSelector::ById(snap_id),
                session: None,
            },
        );
        let body = serde_json::to_vec(&request).unwrap();

        let client = thread::spawn(move || post(addr, &body));
        let (stream, _) = listener.accept().expect("accept");
        transport.serve_one(stream).expect("serve_one");
        client.join().expect("client join");

        let events = log.snapshot();
        assert_eq!(events.len(), 1, "expected exactly one audit event");
        let ev = &events[0];

        // The audit event must have recorded a successful dispatch (the server
        // redacted the node but still returned a result, not an error).
        assert_eq!(
            ev.outcome,
            AuditOutcome::Success,
            "redacted query must still produce Success outcome in audit log"
        );

        // Core guarantee: the secret literal must not appear anywhere in the
        // Debug representation of the event — covering all fields.
        let debug_repr = format!("{ev:?}");
        assert!(
            !debug_repr.contains(SECRET),
            "audit event Debug must not contain the secret value; \
             found in: {debug_repr}"
        );
    }
}
