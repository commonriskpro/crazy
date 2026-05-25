// ── Integration tests: ail-context context response pipeline ─────────────
//
// Spec scenarios covered (tasks 3.1–3.8):
//
//   3.1  Bootstrap: graph bytes in MemoryObjectStore + StoreContextSource roundtrip
//   3.2  Happy-path ContextQuery::Node via InMemoryContextSource
//   3.3  budget = 0 → Err(ContextError::InvalidBudget)
//   3.4  graph_root_hash absent from store → Err(ContextError::Stale)
//   3.5  Queried NodeRef absent from graph → Err(ContextError::NodeNotFound)
//   3.6  context_hash byte-stable for identical structured; distinct inputs differ
//   3.7  Budget exceeded → truncated = true; redaction → redacted = true + node absent
//   3.8  CBOR encode → decode → re-encode produces identical bytes
//
// The tests use `futures::executor::block_on` (sync wrapper) for async calls,
// matching the project-wide convention for integration-test async invocation.

use std::collections::BTreeSet;

use ail_context::source::ContextSource;
use ail_context::{
    ContextError, ContextQuery, ContextResponse, ContextServer, ContextServerConfig,
    FieldRedactionRule, InMemoryContextSource, QueryBudget, QueryScope, ResponseBuilder,
    SnapshotSelector, StoreContextSource, TrustLevel,
};
use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_storage::codec::{CborCodec, ContentCodec};
use ail_storage::graph::GraphStore;
use ail_storage::object::{ObjectId, ObjectStore, RawObject};
use ail_testkit::{
    MemoryObjectStore, ObjectBackedGraphStore, make_semantic_graph, make_snapshot_envelope,
};
use futures::executor::block_on;

// ── Shared helpers ────────────────────────────────────────────────────────

fn no_redactions() -> BTreeSet<NodeRef> {
    BTreeSet::new()
}

// ── 3.1 Bootstrap: StoreContextSource — full roundtrip via real store ────
//
// Spec scenario: "Happy-path materialization"
//   GIVEN a SnapshotEnvelope whose graph_root_hash is present in store
//   WHEN StoreContextSource::load_graph is called
//   THEN it returns Ok(SemanticGraph)
//
// RED: StoreContextSource struct did not exist at the start of PR1.
// GREEN: PR1 impl makes it compile and pass.
#[test]
fn store_source_resolves_snapshot_and_loads_graph() {
    block_on(async {
        let codec = CborCodec;
        let obj_store = MemoryObjectStore::new();
        let graph_store = ObjectBackedGraphStore::new(obj_store.clone());

        let graph = make_semantic_graph();

        // Store graph bytes under a known ObjectId.
        let graph_bytes = codec.encode(&graph).expect("encode graph");
        let graph_id = obj_store
            .put(RawObject(graph_bytes))
            .await
            .expect("put graph bytes");

        // Build a snapshot pointing at the stored graph.
        let mut snap = make_snapshot_envelope("integration-happy");
        snap.graph_root_hash = graph_id;
        graph_store
            .save_snapshot(&snap)
            .await
            .expect("save_snapshot");

        let source = StoreContextSource::new(graph_store, obj_store);

        // resolve_snapshot must return the stored envelope.
        let resolved = source
            .resolve_snapshot(&SnapshotSelector::ById(snap.id))
            .await
            .expect("resolve_snapshot must succeed");
        assert_eq!(resolved.id, snap.id, "resolved snapshot id must match");

        // load_graph must decode and return the stored SemanticGraph.
        let loaded = source
            .load_graph(&graph_id)
            .await
            .expect("load_graph must succeed");
        assert_eq!(
            loaded.nodes.len(),
            graph.nodes.len(),
            "loaded graph must have the same nodes as the stored one"
        );
    });
}

// ── 3.2 Happy-path ContextQuery::Node via InMemoryContextSource ───────────
//
// Spec scenario: "Valid node query is accepted"
//   GIVEN a ContextQuery::Node with a non-zero budget
//   WHEN the query is materialised through InMemoryContextSource + ResponseBuilder
//   THEN no error is returned and structured contains the target node
#[test]
fn in_memory_source_node_query_happy_path() {
    block_on(async {
        let graph = make_semantic_graph();
        let snap = make_snapshot_envelope("node-happy");

        let source = InMemoryContextSource::new();
        source.insert_snapshot(snap.clone());
        source.insert_graph(snap.graph_root_hash, graph.clone());

        let resolved_snap = source
            .resolve_snapshot(&SnapshotSelector::ById(snap.id))
            .await
            .expect("resolve_snapshot must succeed");
        let loaded_graph = source
            .load_graph(&resolved_snap.graph_root_hash)
            .await
            .expect("load_graph must succeed");

        let query = ContextQuery::Node {
            target: NodeRef(0),
            scope: QueryScope::Local,
            budget: QueryBudget::default(),
        };
        let resp = ResponseBuilder::build(&query, &loaded_graph, &resolved_snap, &no_redactions())
            .expect("build must succeed for valid node query");

        assert_eq!(
            resp.structured.len(),
            1,
            "Local scope must return exactly 1 node"
        );
        assert_eq!(
            resp.structured[0].id,
            NodeRef(0),
            "returned node must be the queried target"
        );
        assert!(!resp.truncated, "must not be truncated with max budget");
        assert!(
            !resp.redacted,
            "must not be redacted with empty redaction set"
        );
    });
}

// ── 3.3 Zero budget → InvalidBudget ──────────────────────────────────────
//
// Spec scenario: "Zero-budget query is rejected"
//   GIVEN a ContextQuery::Node with budget = 0
//   WHEN the query is passed to ResponseBuilder::build
//   THEN the system returns Err(ContextError::InvalidBudget)
#[test]
fn zero_budget_query_returns_invalid_budget() {
    let graph = make_semantic_graph();
    let snap = make_snapshot_envelope("zero-budget");
    let query = ContextQuery::Node {
        target: NodeRef(0),
        scope: QueryScope::Local,
        budget: QueryBudget::bytes(0),
    };
    let result = ResponseBuilder::build(&query, &graph, &snap, &no_redactions());
    assert_eq!(
        result,
        Err(ContextError::InvalidBudget),
        "budget = 0 must return Err(InvalidBudget), got: {result:?}"
    );
}

// ── 3.4 graph_root_hash absent → Stale ───────────────────────────────────
//
// Spec scenario: "Stale snapshot returns E_CONTEXT_STALE"
//   GIVEN a SnapshotEnvelope whose graph_root_hash is absent from store
//   WHEN load_graph is called
//   THEN it returns Err(ContextError::Stale)
#[test]
fn load_graph_with_missing_hash_returns_stale() {
    block_on(async {
        let obj_store = MemoryObjectStore::new();
        let graph_store = ObjectBackedGraphStore::new(obj_store.clone());
        let source = StoreContextSource::new(graph_store, obj_store);

        let missing_hash = ObjectId::from_bytes(b"graph-hash-not-in-store");
        let result = source.load_graph(&missing_hash).await;

        assert!(
            matches!(result, Err(ContextError::Stale)),
            "absent graph_root_hash must return Err(Stale), got: {result:?}"
        );
    });
}

// TRIANGULATE: also test that a missing snapshot returns Stale.
#[test]
fn resolve_snapshot_with_missing_id_returns_stale() {
    block_on(async {
        let obj_store = MemoryObjectStore::new();
        let graph_store = ObjectBackedGraphStore::new(obj_store.clone());
        let source = StoreContextSource::new(graph_store, obj_store);

        let missing_id = ObjectId::from_bytes(b"snapshot-id-not-in-store");
        let result = source
            .resolve_snapshot(&SnapshotSelector::ById(missing_id))
            .await;

        assert!(
            matches!(result, Err(ContextError::Stale)),
            "absent snapshot id must return Err(Stale), got: {result:?}"
        );
    });
}

// ── 3.5 Queried NodeRef absent from graph → NodeNotFound ─────────────────
//
// Spec scenario: "Missing node returns E_NODE_NOT_FOUND"
//   GIVEN a materialized SemanticGraph that does not contain the queried NodeRef
//   WHEN a ContextQuery::Node is resolved against it
//   THEN the system returns Err(ContextError::NodeNotFound)
#[test]
fn node_query_for_absent_ref_returns_node_not_found() {
    let graph = make_semantic_graph(); // nodes: 0, 1, 2
    let snap = make_snapshot_envelope("missing-node");
    let query = ContextQuery::Node {
        target: NodeRef(999),
        scope: QueryScope::Local,
        budget: QueryBudget::default(),
    };
    let result = ResponseBuilder::build(&query, &graph, &snap, &no_redactions());
    assert_eq!(
        result,
        Err(ContextError::NodeNotFound),
        "absent NodeRef must return Err(NodeNotFound), got: {result:?}"
    );
}

// ── 3.6 context_hash stability ────────────────────────────────────────────
//
// Spec scenario: "context_hash is stable for identical inputs"
//   GIVEN two ContextResponse values built from the same structured nodes
//   WHEN both context_hash fields are compared
//   THEN they are byte-identical
//
// Spec scenario: "Different structured layers produce different hashes"
//   GIVEN two ContextResponse values with distinct structured node sets
//   WHEN their context_hash fields are compared
//   THEN the hashes differ
#[test]
fn context_hash_is_stable_for_identical_structured() {
    let graph = make_semantic_graph();
    let snap = make_snapshot_envelope("hash-stable");
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };

    let resp_a = ResponseBuilder::build(&query, &graph, &snap, &no_redactions())
        .expect("first build must succeed");
    let resp_b = ResponseBuilder::build(&query, &graph, &snap, &no_redactions())
        .expect("second build must succeed");

    assert_eq!(
        resp_a.context_hash, resp_b.context_hash,
        "identical structured inputs must produce byte-identical context_hash"
    );
}

#[test]
fn distinct_structured_layers_produce_distinct_hashes() {
    use ail_core::semantic_graph::SemanticGraph;

    let snap = make_snapshot_envelope("hash-distinct");
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };

    let graph_a = SemanticGraph {
        nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Module, "alpha")],
        edges: vec![],
    };
    let graph_b = SemanticGraph {
        nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Module, "beta")],
        edges: vec![],
    };

    let resp_a =
        ResponseBuilder::build(&query, &graph_a, &snap, &no_redactions()).expect("build a");
    let resp_b =
        ResponseBuilder::build(&query, &graph_b, &snap, &no_redactions()).expect("build b");

    assert_ne!(
        resp_a.context_hash, resp_b.context_hash,
        "distinct structured layers must produce distinct context_hash values"
    );
}

// ── 3.7 Truncation and redaction flags ───────────────────────────────────
//
// Spec scenario: "Truncation flag set when budget is exceeded"
//   GIVEN a ContextQuery with a budget smaller than the full slice size
//   WHEN the response is built
//   THEN truncated = true and structured contains only nodes within the budget
//
// Spec scenario: "Redaction flag set when nodes are withheld"
//   GIVEN a query over a graph containing at least one node marked for redaction
//   WHEN the response is built
//   THEN redacted = true and the withheld node is absent from structured
#[test]
fn budget_exceeded_sets_truncated_and_limits_structured() {
    let graph = make_semantic_graph(); // 3 nodes
    let snap = make_snapshot_envelope("truncation");
    // budget = 1 byte: too small for any CBOR-encoded node
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::bytes(1),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snap, &no_redactions())
        .expect("build must succeed even with tiny budget");

    assert!(
        resp.truncated,
        "truncated must be true when budget is exceeded before exhausting matches"
    );
    // structured must not contain all 3 nodes (budget too small)
    assert!(
        resp.structured.len() < 3,
        "structured must be smaller than full graph when truncated; got {} nodes",
        resp.structured.len()
    );
}

#[test]
fn redacted_node_is_absent_from_structured_and_flag_is_set() {
    let graph = make_semantic_graph(); // nodes: 0, 1, 2
    let snap = make_snapshot_envelope("redaction");
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let mut redacted_refs = BTreeSet::new();
    redacted_refs.insert(NodeRef(1)); // redact the middle node

    let resp = ResponseBuilder::build(&query, &graph, &snap, &redacted_refs)
        .expect("build must succeed with redaction policy");

    assert!(
        resp.redacted,
        "redacted must be true when at least one node is withheld"
    );
    let ids: Vec<NodeRef> = resp.structured.iter().map(|n| n.id).collect();
    assert!(
        !ids.contains(&NodeRef(1)),
        "redacted node (NodeRef(1)) must be absent from structured; got: {ids:?}"
    );
    assert_eq!(
        ids.len(),
        2,
        "2 of 3 nodes must survive after single-node redaction"
    );
}

// ── 3.8 CBOR determinism: encode → decode → re-encode produces same bytes ──
//
// Spec scenario: "Re-serialization produces identical bytes"
//   GIVEN a ContextResponse serialized to CBOR
//   WHEN the bytes are deserialized and re-serialized
//   THEN the output bytes are identical to the original
#[test]
fn cbor_encode_decode_reencode_produces_identical_bytes() {
    let codec = CborCodec;

    let graph = make_semantic_graph();
    let snap = make_snapshot_envelope("cbor-det");
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snap, &no_redactions())
        .expect("build must succeed");

    let bytes_first = codec.encode(&resp).expect("first encode must succeed");
    let decoded: ContextResponse = codec.decode(&bytes_first).expect("decode must succeed");
    let bytes_second = codec.encode(&decoded).expect("re-encode must succeed");

    assert_eq!(
        bytes_first, bytes_second,
        "CBOR encode → decode → re-encode must produce byte-identical output"
    );
}

// ── 3.9 Redaction guarantee: sensitive body_expr never leaks through JSON ─
//
// Spec: "Redaction is explicit" (context-server.md §Security and redaction)
//       "Summary cannot reveal redacted structured data."
//
//   GIVEN a ContextServer configured to redact `body_expr` for Public trust
//   AND a graph node carrying a sensitive literal value in body_expr
//   WHEN a Graph query is issued without a session (public trust)
//   THEN:
//     - the response is marked redacted
//     - serde_json serialization of the full response does NOT contain the
//       sensitive literal
//     - the summary does NOT contain the sensitive literal
//
// This test pins the full server-to-JSON pipeline, not just ResponseBuilder.
// It is the integration-level guarantee that redaction cannot be bypassed by
// any serialization path added in the future.
//
// RED: no integration-level guarantee test existed for the JSON output path.
// GREEN: ContextServer redaction rules filter body_expr before ResponseBuilder
//        and the resulting JSON is verified to be clean.
#[test]
fn redacted_body_expr_does_not_leak_through_server_to_json() {
    block_on(async {
        const SECRET: &str = "SENSITIVE-CREDENTIAL-xyz987";

        let snap = make_snapshot_envelope("redact-guarantee");
        let mut sensitive = GraphNode::new(NodeRef(0), NodeKind::Function, "payment_handler");
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
                category: "restricted business logic".to_string(),
            }],
            ..Default::default()
        });

        let response = server
            .query(
                &ContextQuery::Graph {
                    scope: QueryScope::Full,
                    budget: QueryBudget::default(),
                },
                &SnapshotSelector::ById(snap.id),
                None, // public session — no auth
            )
            .await
            .expect("query must succeed even with redacted nodes");

        assert!(
            response.redacted,
            "response must be marked redacted when body_expr is withheld"
        );

        // Guarantee: the sensitive literal must not appear anywhere in the
        // JSON-serialized output, including nested fields and the summary.
        let json = serde_json::to_string(&response)
            .expect("JSON serialization of ContextResponse must succeed");
        assert!(
            !json.contains(SECRET),
            "redacted body_expr must not appear in JSON output; \
             sensitive value leaked through serialization: {json}"
        );

        // Guarantee: the summary renderer must also honour redaction.
        assert!(
            !response.summary.contains(SECRET),
            "summary must not reveal the redacted body_expr value"
        );
    });
}
