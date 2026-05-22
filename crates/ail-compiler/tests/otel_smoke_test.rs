// ── ail-compiler::otel_smoke_test ────────────────────────────────────────
//
// Smoke test: verify that `compile_incremental` can be called inside a
// `tracing` span without panicking, regardless of whether the `otel` feature
// is active.  No subscriber is required — the no-op default is enough.

use ail_compiler::{MemoryArtifactCache, NodeHashes, compile_incremental};
use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::report::VerificationReport;

fn minimal_graph() -> SemanticGraph {
    SemanticGraph {
        nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Function, "smoke_fn")],
        edges: vec![],
    }
}

fn proven_report() -> VerificationReport {
    VerificationReport { entries: vec![], ..Default::default() }
}

/// Instruments a `compile_incremental` call inside a tracing span and asserts
/// no panic occurs.  The test passes whether or not a subscriber is installed —
/// the point is that the `#[cfg_attr(feature = "otel", tracing::instrument)]`
/// attribute does not break the function's call semantics.
#[test]
fn compile_incremental_does_not_panic_inside_span() {
    let graph = minimal_graph();
    let report = proven_report();
    let cache = MemoryArtifactCache::new();
    // Empty prev_hashes → all nodes are dirty → first-compile path.
    let prev_hashes = NodeHashes::default();

    let span = tracing::span!(tracing::Level::INFO, "otel_smoke");
    let _guard = span.enter();

    let result = compile_incremental(&graph, &report, &cache, &prev_hashes);

    // The call must succeed (proven report, valid graph).
    assert!(result.is_ok(), "compile_incremental failed inside span: {result:?}");
}
