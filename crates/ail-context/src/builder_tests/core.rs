use super::*;

// ── zero_budget_returns_invalid_budget ────────────────────────────────
// Spec scenario: "Zero-budget query is rejected".
//
// RED: `ResponseBuilder::build` did not exist → compile error.
// GREEN: budget == 0 guard at the start of build() makes it pass.
#[test]
fn zero_budget_returns_invalid_budget() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::bytes(0),
    };
    let result = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions());
    assert_eq!(
        result,
        Err(ContextError::InvalidBudget),
        "budget = 0 must return Err(InvalidBudget)"
    );
}

// ── node_query_local_returns_target_only ──────────────────────────────
// Spec scenario: "Valid node query is accepted".
#[test]
fn node_query_local_returns_target_only() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let query = ContextQuery::Node {
        target: NodeRef(0),
        scope: QueryScope::Local,
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("build must succeed");
    assert_eq!(resp.structured.len(), 1, "Local scope must return 1 node");
    assert_eq!(
        resp.structured[0].id,
        NodeRef(0),
        "Local scope must return the target node"
    );
    assert!(!resp.truncated, "must not be truncated with max budget");
    assert!(!resp.redacted, "must not be redacted with empty set");
}

// ── node_query_full_returns_all_reachable ─────────────────────────────
// Spec: Full scope traverses BFS from target.
// TRIANGULATE: forces real BFS logic (Local test alone would not).
#[test]
fn node_query_full_returns_all_reachable() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let query = ContextQuery::Node {
        target: NodeRef(0),
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("build must succeed");
    // 0 → 1 → 2: all three are reachable from 0
    assert_eq!(
        resp.structured.len(),
        3,
        "Full scope from root must reach all 3 nodes; got {:?}",
        resp.structured.iter().map(|n| n.id).collect::<Vec<_>>()
    );
}

// ── node_query_missing_target_returns_node_not_found ──────────────────
// Spec scenario: "Missing node returns E_NODE_NOT_FOUND".
#[test]
fn node_query_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let query = ContextQuery::Node {
        target: NodeRef(99),
        scope: QueryScope::Local,
        budget: QueryBudget::default(),
    };
    let result = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions());
    assert_eq!(
        result,
        Err(ContextError::NodeNotFound),
        "missing target must return Err(NodeNotFound)"
    );
}

// ── context_hash_stable_for_identical_inputs ──────────────────────────
// Spec scenario: "context_hash is stable for identical inputs".
#[test]
fn context_hash_stable_for_identical_inputs() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let resp_a =
        ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions()).expect("first build");
    let resp_b =
        ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions()).expect("second build");
    assert_eq!(
        resp_a.context_hash, resp_b.context_hash,
        "identical inputs must produce identical context_hash"
    );
}

// ── different_inputs_produce_different_hashes ─────────────────────────
// Spec scenario: "Different structured layers produce different hashes".
// TRIANGULATE: forces real hash logic (not a hardcoded constant).
#[test]
fn different_inputs_produce_different_hashes() {
    let snapshot = make_snapshot();

    let graph_a = SemanticGraph {
        nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Module, "a")],
        edges: vec![],
    };
    let graph_b = SemanticGraph {
        nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Module, "b")],
        edges: vec![],
    };
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };

    let resp_a = ResponseBuilder::build(&query, &graph_a, &snapshot, &no_redactions()).expect("a");
    let resp_b = ResponseBuilder::build(&query, &graph_b, &snapshot, &no_redactions()).expect("b");

    assert_ne!(
        resp_a.context_hash, resp_b.context_hash,
        "distinct structured layers must produce distinct context_hash"
    );
}

// ── budget_exceeded_sets_truncated ────────────────────────────────────
// Spec scenario: "Truncation flag set when budget is exceeded".
#[test]
fn budget_exceeded_sets_truncated() {
    let graph = make_graph(); // 3 nodes
    let snapshot = make_snapshot();
    // budget = 1 byte: definitely smaller than any CBOR-encoded node
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::bytes(1),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("build must succeed even with tiny budget");
    assert!(
        resp.truncated,
        "structured layer exceeding budget must set truncated = true"
    );
}

// ── redacted_node_absent_from_structured ──────────────────────────────
// Spec scenario: "Redaction flag set when nodes are withheld".
#[test]
fn redacted_node_absent_from_structured() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let mut redacted_refs = BTreeSet::new();
    redacted_refs.insert(NodeRef(1)); // redact the middle node

    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &redacted_refs)
        .expect("build must succeed");
    assert!(
        resp.redacted,
        "redacted flag must be true when a node is withheld"
    );
    let ids: Vec<NodeRef> = resp.structured.iter().map(|n| n.id).collect();
    assert!(
        !ids.contains(&NodeRef(1)),
        "redacted node must be absent from structured; got: {ids:?}"
    );
    assert_eq!(ids.len(), 2, "2 of 3 nodes survive redaction");
}

// ── TRIANGULATE: graph_query_full_includes_all_nodes ─────────────────
// Different from node_query_full: exercises the Graph branch of collect_candidates.
#[test]
fn graph_query_full_includes_all_nodes() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("build must succeed");
    assert_eq!(
        resp.structured.len(),
        3,
        "Graph + Full must include all 3 nodes"
    );
    // Verify NodeRef order
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert_eq!(ids, vec![0, 1, 2], "nodes must be sorted by NodeRef");
}

// ── freshness_equals_snapshot_created_at ─────────────────────────────
// Spec: `freshness` is `snapshot.created_at`.
#[test]
fn freshness_equals_snapshot_created_at() {
    let graph = make_graph();
    let snapshot = make_snapshot(); // created_at = 1_000
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("build must succeed");
    assert_eq!(
        resp.freshness, 1_000,
        "freshness must equal snapshot.created_at"
    );
}

// ── response_has_schema_and_query_hash ────────────────────────────────
// Spec: every response carries schema version and a stable query_hash.
#[test]
fn response_has_schema_and_query_hash() {
    use crate::dto::CONTEXT_SCHEMA_V1;
    let graph = make_graph();
    let snapshot = make_snapshot();
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("build must succeed");
    assert_eq!(resp.schema, CONTEXT_SCHEMA_V1, "schema must be context/1.0");
    // query_hash must be non-zero (blake3 of CBOR(query))
    assert_ne!(
        resp.query_hash, [0u8; 32],
        "query_hash must not be the zero array"
    );
}

// ── query_hash_stable_for_identical_query ─────────────────────────────
// TRIANGULATE: same query → same query_hash.
#[test]
fn query_hash_stable_for_identical_query() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let query = ContextQuery::Graph {
        scope: QueryScope::Local,
        budget: QueryBudget::bytes(1024),
    };
    let resp_a =
        ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions()).expect("first build");
    let resp_b =
        ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions()).expect("second build");
    assert_eq!(
        resp_a.query_hash, resp_b.query_hash,
        "identical query must produce identical query_hash"
    );
}

// ── different_queries_produce_different_query_hashes ──────────────────
// Two distinct queries must produce distinct query_hash values.
#[test]
fn different_queries_produce_different_query_hashes() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let q1 = ContextQuery::Graph {
        scope: QueryScope::Local,
        budget: QueryBudget::bytes(1024),
    };
    let q2 = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::bytes(1024),
    };
    let resp1 = ResponseBuilder::build(&q1, &graph, &snapshot, &no_redactions()).unwrap();
    let resp2 = ResponseBuilder::build(&q2, &graph, &snapshot, &no_redactions()).unwrap();
    assert_ne!(
        resp1.query_hash, resp2.query_hash,
        "distinct queries must produce distinct query_hash values"
    );
}

// ── response_has_limits_block ─────────────────────────────────────────
// Spec: response must carry a limits block with budget_bytes and bytes_used.
#[test]
fn response_has_limits_block() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("build must succeed");
    assert_eq!(resp.limits.budget_bytes, usize::MAX);
    assert!(!resp.limits.truncated, "not truncated with max budget");
    assert!(
        resp.limits.bytes_used > 0,
        "bytes_used must be > 0 for non-empty graph"
    );
}
