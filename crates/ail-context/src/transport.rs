// ── ail-context::transport ────────────────────────────────────────────────
//
// Newline-delimited JSON-RPC stdio transport for ContextServer.
//
// This module provides the I/O framing layer that sits above
// ContextServer::handle_rpc.  It reads newline-delimited JSON requests from
// any BufRead source, dispatches each through the in-process server, and
// writes newline-delimited JSON responses to any Write sink.
//
// The transport is deliberately I/O-agnostic: stdin/stdout, named pipes,
// Unix sockets, or in-memory Cursor buffers all satisfy the same interface.
// This matches the decision-log principle: "stdio/MCP-like before HTTP;
// distributed auth can follow after the protocol proves useful."
//
// # Protocol
//
// - Each request is a single JSON object on one line (no pretty-printing).
// - Each response is a single JSON object followed by `\n`.
// - Empty lines are ignored.
// - Malformed JSON produces a JSON-RPC parse-error response (`-32700`).
//   The response id is `""` (not the spec-required `null`) because
//   `ContextRpcResponse::id` is typed as `String`, which has no null
//   representation.  See [`server::ContextRpcResponse::parse_error`] for
//   the full rationale.  Clients MUST tolerate `"id": ""` on parse errors.
// - Read I/O errors stop the serve loop with `TransportError::Read`.
// - Write I/O errors stop the serve loop with `TransportError::Write`.

use std::io::{BufRead, Write};

use futures::executor::block_on;

use crate::server::{ContextRpcRequest, ContextRpcResponse, ContextServer};
use crate::source::ContextSource;

// ── TransportError ────────────────────────────────────────────────────────

/// Errors that can occur in the stdio transport framing layer.
///
/// These are distinct from [`ContextError`][crate::error::ContextError]:
/// transport errors are I/O or framing failures, not semantic query failures.
/// Semantic failures (stale context, budget exceeded, etc.) are returned as
/// JSON-RPC error responses inside the normal response envelope.
#[derive(Debug)]
pub enum TransportError {
    /// I/O failure reading from the input source.
    Read(std::io::Error),
    /// I/O failure writing to the output sink.
    Write(std::io::Error),
    /// The response could not be serialized to JSON.
    ///
    /// This should not occur in practice because all response types derive
    /// `Serialize`; it is retained as a safety net against future changes.
    Encode(String),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportError::Read(e) => write!(f, "transport read error: {e}"),
            TransportError::Write(e) => write!(f, "transport write error: {e}"),
            TransportError::Encode(msg) => write!(f, "transport encode error: {msg}"),
        }
    }
}

impl std::error::Error for TransportError {}

// ── StdioTransport ────────────────────────────────────────────────────────

/// Newline-delimited JSON-RPC transport over any `BufRead`/`Write` pair.
///
/// The transport owns a [`ContextServer`] and serves requests sequentially:
/// one line in, one JSON-RPC response line out.  AI tooling (Claude, Cursor,
/// etc.) can drive the Context Server via stdin/stdout without adding a
/// network server to `ail-context`.
///
/// # Example
///
/// ```rust,ignore
/// use ail_context::{StdioTransport, ContextServer};
/// use ail_context::source::InMemoryContextSource;
/// use std::io::{BufReader, stdin, stdout};
///
/// let server = ContextServer::new(InMemoryContextSource::new());
/// let transport = StdioTransport::new(server);
/// transport.serve(BufReader::new(stdin()), &mut stdout()).unwrap();
/// ```
pub struct StdioTransport<S> {
    server: ContextServer<S>,
}

impl<S> StdioTransport<S> {
    /// Create a transport that wraps the given server.
    pub fn new(server: ContextServer<S>) -> Self {
        Self { server }
    }

    /// Return a reference to the inner server.
    pub fn server(&self) -> &ContextServer<S> {
        &self.server
    }
}

impl<S> StdioTransport<S>
where
    S: ContextSource + Send + Sync,
{
    /// Serve requests from `reader`, writing responses to `writer`.
    ///
    /// Reads until EOF; returns `Ok(())` when the reader is exhausted.
    /// Returns `Err(TransportError::Read)` on read I/O failures.
    /// Returns `Err(TransportError::Write)` on write I/O failures.
    ///
    /// Parse errors in individual request lines are returned to the client as
    /// JSON-RPC error responses (`-32700`) rather than stopping the loop.
    pub fn serve<R: BufRead, W: Write>(
        &self,
        reader: R,
        writer: &mut W,
    ) -> Result<(), TransportError> {
        for line_result in reader.lines() {
            let line = line_result.map_err(TransportError::Read)?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let response = self.handle_line(trimmed);
            self.write_response(writer, &response)?;
        }
        Ok(())
    }

    /// Parse one trimmed non-empty line and dispatch it through the server.
    fn handle_line(&self, line: &str) -> ContextRpcResponse {
        match serde_json::from_str::<ContextRpcRequest>(line) {
            Ok(request) => block_on(self.server.handle_rpc(request)),
            Err(e) => ContextRpcResponse::parse_error(e.to_string()),
        }
    }

    /// Serialize a response to a newline-terminated JSON line and flush.
    ///
    /// The flush is mandatory: when `writer` is a buffered sink (e.g.
    /// `BufWriter<Stdout>`), omitting it leaves the response in the kernel
    /// buffer and the client hangs indefinitely waiting for bytes that never
    /// arrive.  For unbuffered or in-memory sinks the flush is a no-op.
    fn write_response<W: Write>(
        &self,
        writer: &mut W,
        response: &ContextRpcResponse,
    ) -> Result<(), TransportError> {
        let bytes =
            serde_json::to_vec(response).map_err(|e| TransportError::Encode(e.to_string()))?;
        writer.write_all(&bytes).map_err(TransportError::Write)?;
        writer.write_all(b"\n").map_err(TransportError::Write)?;
        writer.flush().map_err(TransportError::Write)?;
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
    use ail_storage::graph::SnapshotEnvelope;
    use ail_storage::object::ObjectId;

    use super::*;
    use crate::dto::{ContextQuery, SnapshotSelector};
    use crate::server::{
        CONTEXT_RPC_AUTH_METHOD, CONTEXT_RPC_QUERY_METHOD, ContextRequest, ContextResponse,
        JSONRPC_INVALID_PARAMS, JSONRPC_METHOD_NOT_FOUND, JSONRPC_PARSE_ERROR,
    };
    use crate::source::InMemoryContextSource;
    use crate::{ContextRpcRequest, QueryBudget, QueryScope};

    // ── Test helpers ──────────────────────────────────────────────────────

    fn snapshot() -> SnapshotEnvelope {
        let id = ObjectId::from_bytes(b"transport-snapshot");
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
        let node = GraphNode::new(NodeRef(0), NodeKind::Function, "transport_fn");
        SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        }
    }

    fn make_transport() -> StdioTransport<InMemoryContextSource> {
        let snap = snapshot();
        let g = graph();
        let source = InMemoryContextSource::new();
        source.insert_snapshot(snap.clone());
        source.insert_graph(snap.graph_root_hash, g);
        StdioTransport::new(ContextServer::new(source))
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

    /// Serve a multi-line string through the transport and collect output.
    fn serve_str(transport: &StdioTransport<InMemoryContextSource>, input: &str) -> String {
        let reader = Cursor::new(input.as_bytes());
        let mut output = Vec::new();
        transport
            .serve(reader, &mut output)
            .expect("serve must not return Err for well-behaved I/O");
        String::from_utf8(output).expect("output must be valid UTF-8")
    }

    /// Parse every non-empty output line as a `ContextRpcResponse`.
    fn parse_responses(output: &str) -> Vec<ContextRpcResponse> {
        output
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("each line must be valid JSON-RPC"))
            .collect()
    }

    // ── single_valid_query_returns_result ─────────────────────────────────
    // Spec: a well-formed context.query request produces a Result response.
    //
    // RED: transport module did not exist.
    // GREEN: StdioTransport::serve dispatches through handle_rpc.
    #[test]
    fn single_valid_query_returns_result() {
        let t = make_transport();
        let request = valid_query_request("t-1");
        let input = serde_json::to_string(&request).unwrap() + "\n";

        let output = serve_str(&t, &input);
        let responses = parse_responses(&output);

        assert_eq!(responses.len(), 1);
        let resp = &responses[0];
        assert_eq!(resp.id, "t-1");
        assert!(
            resp.error.is_none(),
            "unexpected RPC error: {:?}",
            resp.error
        );
        assert!(
            matches!(resp.result, Some(ContextResponse::Result(_))),
            "expected Result variant, got: {:?}",
            resp.result
        );
    }

    // ── malformed_json_returns_parse_error ────────────────────────────────
    // Spec: an unparseable line returns a JSON-RPC -32700 error response.
    //       The serve loop must continue after a parse error.
    //
    // RED: no transport to emit parse errors.
    // GREEN: handle_line emits ContextRpcResponse::parse_error.
    #[test]
    fn malformed_json_returns_parse_error() {
        let t = make_transport();
        let output = serve_str(&t, "{not valid json\n");
        let responses = parse_responses(&output);

        assert_eq!(responses.len(), 1);
        let resp = &responses[0];
        assert!(
            resp.result.is_none(),
            "result must be absent for parse errors"
        );
        let err = resp.error.as_ref().expect("error field must be present");
        assert_eq!(
            err.code, JSONRPC_PARSE_ERROR,
            "parse error must use code {JSONRPC_PARSE_ERROR}"
        );
    }

    // ── empty_lines_are_skipped ───────────────────────────────────────────
    // Spec: blank lines (whitespace-only) produce no output and do not crash.
    //
    // RED: no transport.
    // GREEN: serve skips empty trimmed lines.
    #[test]
    fn empty_lines_are_skipped() {
        let t = make_transport();
        let request = valid_query_request("t-3");
        let line = serde_json::to_string(&request).unwrap();
        let input = format!("\n  \n{line}\n\n");

        let output = serve_str(&t, &input);
        let responses = parse_responses(&output);

        assert_eq!(responses.len(), 1, "empty lines must not produce responses");
        assert_eq!(responses[0].id, "t-3");
    }

    // ── unknown_method_returns_method_not_found ───────────────────────────
    // Spec: a valid JSON-RPC envelope with an unknown method returns -32601.
    //
    // RED: no transport.
    // GREEN: handle_rpc returns method-not-found; transport frames it.
    #[test]
    fn unknown_method_returns_method_not_found() {
        let t = make_transport();
        let snap = snapshot();
        let request = ContextRpcRequest::new(
            "t-4",
            "context.search",
            ContextRequest::Query {
                query: ContextQuery::Graph {
                    scope: QueryScope::Full,
                    budget: QueryBudget::default(),
                },
                snapshot: SnapshotSelector::ById(snap.id),
                session: None,
            },
        );
        let input = serde_json::to_string(&request).unwrap() + "\n";

        let output = serve_str(&t, &input);
        let responses = parse_responses(&output);

        assert_eq!(responses.len(), 1);
        let err = responses[0].error.as_ref().expect("error must be set");
        assert_eq!(
            err.code, JSONRPC_METHOD_NOT_FOUND,
            "unknown method must return {JSONRPC_METHOD_NOT_FOUND}"
        );
    }

    // ── multiple_requests_each_produce_a_response ─────────────────────────
    // Spec: N requests produce exactly N responses in order.
    //
    // RED: no transport.
    // GREEN: serve loops over all lines.
    #[test]
    fn multiple_requests_each_produce_a_response() {
        let t = make_transport();
        let r1 = valid_query_request("t-5a");
        let r2 = valid_query_request("t-5b");
        let input = format!(
            "{}\n{}\n",
            serde_json::to_string(&r1).unwrap(),
            serde_json::to_string(&r2).unwrap(),
        );

        let output = serve_str(&t, &input);
        let responses = parse_responses(&output);

        assert_eq!(responses.len(), 2, "two requests must yield two responses");
        assert_eq!(responses[0].id, "t-5a");
        assert_eq!(responses[1].id, "t-5b");
    }

    // ── method_payload_mismatch_returns_invalid_params ────────────────────
    // Spec: a known method with a mismatched payload variant returns -32602.
    //
    // RED: no transport.
    // GREEN: handle_rpc enforces method/payload contract; transport frames it.
    #[test]
    fn method_payload_mismatch_returns_invalid_params() {
        let t = make_transport();
        let snap = snapshot();
        // context.auth method, but Query payload — a mismatch.
        let request = ContextRpcRequest::new(
            "t-6",
            CONTEXT_RPC_AUTH_METHOD,
            ContextRequest::Query {
                query: ContextQuery::Graph {
                    scope: QueryScope::Full,
                    budget: QueryBudget::default(),
                },
                snapshot: SnapshotSelector::ById(snap.id),
                session: None,
            },
        );
        let input = serde_json::to_string(&request).unwrap() + "\n";

        let output = serve_str(&t, &input);
        let responses = parse_responses(&output);

        assert_eq!(responses.len(), 1);
        let err = responses[0].error.as_ref().expect("error must be set");
        assert_eq!(
            err.code, JSONRPC_INVALID_PARAMS,
            "method/payload mismatch must return {JSONRPC_INVALID_PARAMS}"
        );
    }

    // ── empty_input_produces_no_responses ─────────────────────────────────
    // Spec: an empty reader produces no responses and returns Ok(()).
    //
    // RED: no transport.
    // GREEN: serve loop exits cleanly at EOF.
    #[test]
    fn empty_input_produces_no_responses() {
        let t = make_transport();
        let output = serve_str(&t, "");
        let responses = parse_responses(&output);
        assert_eq!(responses.len(), 0, "empty input must produce no responses");
    }

    // ── parse_error_response_is_valid_json_rpc ────────────────────────────
    // Spec: the parse-error response itself is valid JSON-RPC and can round-trip.
    //
    // RED: no transport / parse_error constructor.
    // GREEN: parse_error emits a well-formed ContextRpcResponse.
    #[test]
    fn parse_error_response_is_valid_json_rpc() {
        let t = make_transport();
        let output = serve_str(&t, "totally-not-json\n");
        let responses = parse_responses(&output);

        assert_eq!(responses.len(), 1);
        let resp = &responses[0];
        assert!(resp.result.is_none());
        let err = resp.error.as_ref().expect("error must be set");
        assert_eq!(err.code, JSONRPC_PARSE_ERROR);

        // Verify the response round-trips through JSON without loss.
        let reencoded = serde_json::to_string(resp).expect("must re-encode");
        let redecoded: ContextRpcResponse =
            serde_json::from_str(&reencoded).expect("must re-decode");
        assert_eq!(
            err.code,
            redecoded.error.as_ref().unwrap().code,
            "parse-error code must survive JSON round-trip"
        );
    }

    // ── flush_called_once_per_response ────────────────────────────────────
    // Spec: write_response flushes after every response so that buffered
    //       writers (BufWriter<Stdout>) deliver bytes to the client
    //       immediately rather than accumulating them indefinitely.
    //
    // RED: write_response omitted the flush call.
    // GREEN: writer.flush() added after write_all(b"\n").
    #[test]
    fn flush_called_once_per_response() {
        struct FlushCountingWriter {
            inner: Vec<u8>,
            flush_count: usize,
        }

        impl std::io::Write for FlushCountingWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.inner.write(buf)
            }

            fn flush(&mut self) -> std::io::Result<()> {
                self.flush_count += 1;
                Ok(())
            }
        }

        let t = make_transport();
        let r1 = valid_query_request("t-flush-1");
        let r2 = valid_query_request("t-flush-2");
        let input = format!(
            "{}\n{}\n",
            serde_json::to_string(&r1).unwrap(),
            serde_json::to_string(&r2).unwrap(),
        );

        let reader = Cursor::new(input.as_bytes());
        let mut writer = FlushCountingWriter {
            inner: Vec::new(),
            flush_count: 0,
        };
        t.serve(reader, &mut writer)
            .expect("serve must not error for well-behaved I/O");

        assert_eq!(
            writer.flush_count, 2,
            "flush must be called exactly once per response (once per request here)"
        );
    }

    // ── parse_error_then_valid_request_both_produce_responses ─────────────
    // Spec: a parse error does not stop the serve loop; subsequent valid
    //       requests still get processed.
    //
    // RED: no transport.
    // GREEN: serve loops past parse errors via the handle_line path.
    #[test]
    fn parse_error_then_valid_request_both_produce_responses() {
        let t = make_transport();
        let request = valid_query_request("t-8");
        let input = format!(
            "{{bad json}}\n{}\n",
            serde_json::to_string(&request).unwrap()
        );

        let output = serve_str(&t, &input);
        let responses = parse_responses(&output);

        assert_eq!(
            responses.len(),
            2,
            "both the parse error and the valid request must produce responses"
        );
        assert_eq!(
            responses[0].error.as_ref().unwrap().code,
            JSONRPC_PARSE_ERROR
        );
        assert_eq!(responses[1].id, "t-8");
        assert!(responses[1].error.is_none());
    }
}
