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

mod audit;
mod error;
mod framing;
mod peer;
mod transport;

pub use audit::{AuditOutcome, QueryAuditEvent, QueryAuditLog};
pub use error::HttpTransportError;
pub use transport::{
    HTTP_MAX_BODY_BYTES, HTTP_MAX_HEADER_BYTES, HTTP_READ_TIMEOUT, HTTP_WRITE_TIMEOUT,
    HttpTransport,
};

use framing::write_http_response;
use peer::check_peer_addr;

#[cfg(test)]
mod tests;
