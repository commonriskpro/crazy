use std::sync::{Arc, Mutex};

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

    pub(super) fn record(&self, event: QueryAuditEvent) {
        self.0.lock().unwrap().push(event);
    }
}
