use std::io::{BufRead, BufReader, Read};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use futures::executor::block_on;

use crate::server::{ContextRpcRequest, ContextRpcResponse, ContextServer};
use crate::source::ContextSource;

use super::{
    AuditOutcome, HttpTransportError, QueryAuditEvent, QueryAuditLog, check_peer_addr,
    write_http_response,
};

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
