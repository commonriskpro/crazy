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
    CONTEXT_RPC_QUERY_METHOD, ContextRequest, ContextResponse, ContextServer, JSONRPC_PARSE_ERROR,
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
