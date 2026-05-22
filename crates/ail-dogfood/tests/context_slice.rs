// ── ail-dogfood::tests::context_slice ────────────────────────────────────
//
// Integration test: ail-context's ResponseBuilder can produce a non-empty
// bounded context slice from the self-referential graph.
//
// # Spec scenario: "Context builder produces non-empty summary"
//
//   GIVEN the self-model graph is passed to the context builder
//   WHEN a context summary is requested for node "meta" (NodeRef(0))
//   THEN the summary is non-empty (has at least one entry)
//   AND no panic or unhandled error occurs

use std::collections::BTreeSet;

use ail_context::builder::ResponseBuilder;
use ail_context::dto::{ContextQuery, QueryScope};
use ail_core::semantic_graph::NodeRef;
use ail_dogfood::graph_self_model::build_graph_self_model;
use ail_storage::graph::SnapshotEnvelope;
use ail_storage::object::ObjectId;

// ── stub_snapshot ─────────────────────────────────────────────────────────

/// Construct a minimal in-memory `SnapshotEnvelope` with a stub hash.
///
/// Design decision: `ResponseBuilder::build()` only reads `graph_root_hash`
/// and `created_at` from the envelope.  A stub is sufficient for dogfood
/// purposes and avoids any I/O in tests.
fn stub_snapshot() -> SnapshotEnvelope {
    let id = ObjectId::from_bytes(b"dogfood-context-slice-test-snap");
    SnapshotEnvelope {
        id,
        graph_root_hash: id,
        parent_id: None,
        applied_change_id: None,
        created_at: 0,
        verification_report_hash: None,
        ..Default::default()
    }
}

// ── context_builder_produces_non_empty_summary_from_self_model ────────────
// Spec scenario: "Context builder produces non-empty summary"
//   GIVEN the self-model graph is passed to the context builder
//   WHEN a context summary is requested for node "meta" with Full scope
//   THEN the summary is non-empty
//   AND no panic or unhandled error occurs
#[test]
fn context_builder_produces_non_empty_summary_from_self_model() {
    let graph = build_graph_self_model();
    let snapshot = stub_snapshot();
    let redacted: BTreeSet<NodeRef> = BTreeSet::new();

    // Query centered on NodeRef(0) = the "meta" module node, Full scope.
    // BFS from meta follows DependsOn edges to all 9 type nodes → 10 total.
    let query = ContextQuery::Node {
        target: NodeRef(0),
        scope: QueryScope::Full,
        budget: usize::MAX,
    };

    let response = ResponseBuilder::build(&query, &graph, &snapshot, &redacted)
        .expect("ResponseBuilder::build must succeed for a valid self-model graph");

    assert!(
        !response.summary.is_empty(),
        "context summary must be non-empty for a 10-node self-model graph; got an empty string"
    );

    assert!(
        !response.structured.is_empty(),
        "structured layer must be non-empty; got 0 nodes"
    );
}
