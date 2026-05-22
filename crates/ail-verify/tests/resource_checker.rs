// ── ail-verify::resource_checker tests ───────────────────────────────────
//
// Strict TDD — tests for resource lifecycle verification.
// Spec: verification-pipeline/spec §1 (resource lifecycle checker).

use ail_core::semantic_graph::{
    EdgeKind, GraphEdge, GraphNode, NodeKind, NodeRef, SemanticGraph, TrustMetadata,
};
use ail_verify::report::VerificationState;
use ail_verify::resource_checker::ResourceChecker;

fn resource_node(id: u32, name: &str, level: &str, tags: Vec<&str>) -> GraphNode {
    let mut node = GraphNode::new(NodeRef(id), NodeKind::Type, name);
    node.trust_metadata = Some(TrustMetadata {
        level: level.to_string(),
        tags: tags.into_iter().map(String::from).collect(),
    });
    node
}

fn graph(nodes: Vec<GraphNode>) -> SemanticGraph {
    SemanticGraph {
        nodes,
        edges: vec![],
    }
}

// ── Scenario: non-resource nodes are skipped ──────────────────────────────
// GIVEN a graph with nodes that have no trust_metadata (or non-resource levels)
// WHEN ResourceChecker::check is called
// THEN no entries are emitted
#[test]
fn non_resource_nodes_produce_no_entries() {
    let g = graph(vec![
        GraphNode::new(NodeRef(0), NodeKind::Function, "fn_a"),
        GraphNode::new(NodeRef(1), NodeKind::Module, "mod_b"),
    ]);
    let report = ResourceChecker::check(&g);
    assert!(report.entries.is_empty());
}

// ── Scenario: use-after-release → Failed ─────────────────────────────────
// GIVEN a linear resource with "released" AND "use-after-release" tags
// WHEN ResourceChecker::check is called
// THEN the entry state is Failed
#[test]
fn use_after_release_is_failed() {
    let g = graph(vec![resource_node(
        0,
        "db_conn",
        "resource:linear",
        vec!["released", "use-after-release"],
    )]);
    let report = ResourceChecker::check(&g);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].state, VerificationState::Failed);
    assert!(
        report.entries[0]
            .evidence
            .as_ref()
            .unwrap()
            .contains("use-after-release")
    );
}

// ── Scenario: double-release → Failed ────────────────────────────────────
// GIVEN an affine resource with "double-release" tag
// WHEN ResourceChecker::check is called
// THEN the entry state is Failed
#[test]
fn double_release_is_failed() {
    let g = graph(vec![resource_node(
        0,
        "file_handle",
        "resource:affine",
        vec!["released", "double-release"],
    )]);
    let report = ResourceChecker::check(&g);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].state, VerificationState::Failed);
}

// ── Scenario: linear never-consumed → Failed ─────────────────────────────
// GIVEN a linear resource with "never-consumed" tag
// WHEN ResourceChecker::check is called
// THEN the entry state is Failed
#[test]
fn linear_never_consumed_is_failed() {
    let g = graph(vec![resource_node(
        0,
        "tx",
        "resource:linear",
        vec!["never-consumed"],
    )]);
    let report = ResourceChecker::check(&g);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].state, VerificationState::Failed);
    assert!(
        report.entries[0]
            .evidence
            .as_ref()
            .unwrap()
            .contains("never consumed")
    );
}

// ── Scenario: linear properly released → Proven ───────────────────────────
// GIVEN a linear resource with "released" tag (and no violation tags)
// WHEN ResourceChecker::check is called
// THEN the entry state is Proven
#[test]
fn linear_released_is_proven() {
    let g = graph(vec![resource_node(
        0,
        "lock",
        "resource:linear",
        vec!["released"],
    )]);
    let report = ResourceChecker::check(&g);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].state, VerificationState::Proven);
}

// ── Scenario: affine resource with lock-released → Proven ─────────────────
// GIVEN an affine resource with "lock-released" tag
// WHEN ResourceChecker::check is called
// THEN the entry state is Proven
#[test]
fn affine_lock_released_is_proven() {
    let g = graph(vec![resource_node(
        0,
        "mutex_guard",
        "resource:affine",
        vec!["lock-released"],
    )]);
    let report = ResourceChecker::check(&g);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].state, VerificationState::Proven);
}

// ── Scenario: affine transaction committed → Proven ───────────────────────
#[test]
fn affine_transaction_committed_is_proven() {
    let g = graph(vec![resource_node(
        0,
        "db_tx",
        "resource:affine",
        vec!["transaction-committed"],
    )]);
    let report = ResourceChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Proven);
}

// ── Scenario: shared without safe-capability → Unsafe ─────────────────────
// GIVEN a shared resource WITHOUT "safe-capability" tag
// WHEN ResourceChecker::check is called
// THEN the entry state is Unsafe
#[test]
fn shared_without_safe_capability_is_unsafe() {
    let g = graph(vec![resource_node(
        0,
        "shared_counter",
        "resource:shared",
        vec![], // no safe-capability
    )]);
    let report = ResourceChecker::check(&g);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].state, VerificationState::Unsafe);
}

// ── Scenario: shared with safe-capability → Proven ────────────────────────
// GIVEN a shared resource WITH "safe-capability" tag
// WHEN ResourceChecker::check is called
// THEN the entry state is Proven
#[test]
fn shared_with_safe_capability_is_proven() {
    let g = graph(vec![resource_node(
        0,
        "arc_mutex",
        "resource:shared",
        vec!["safe-capability"],
    )]);
    let report = ResourceChecker::check(&g);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].state, VerificationState::Proven);
}

// ── TRIANGULATE: multiple resource nodes, mixed states ────────────────────
// GIVEN a graph with linear (ok), shared (bad), affine (ok)
// WHEN ResourceChecker::check is called
// THEN 3 entries are produced with correct individual states
#[test]
fn multiple_resource_nodes_each_get_entry() {
    let g = graph(vec![
        resource_node(0, "linear_ok", "resource:linear", vec!["released"]),
        resource_node(1, "shared_bad", "resource:shared", vec![]),
        resource_node(2, "affine_ok", "resource:affine", vec!["released"]),
    ]);
    let report = ResourceChecker::check(&g);
    assert_eq!(report.entries.len(), 3);
    assert_eq!(report.entries[0].state, VerificationState::Proven);
    assert_eq!(report.entries[1].state, VerificationState::Unsafe);
    assert_eq!(report.entries[2].state, VerificationState::Proven);
}

// ── TRIANGULATE: summary reflects worst state ─────────────────────────────
#[test]
fn summary_reflects_worst_state_across_resources() {
    let g = graph(vec![
        resource_node(0, "ok", "resource:linear", vec!["released"]),
        resource_node(1, "bad", "resource:linear", vec!["use-after-release"]),
    ]);
    let report = ResourceChecker::check(&g);
    assert_eq!(report.summary(), VerificationState::Failed);
}

// ── TRIANGULATE: diagnostics emitted for violations ───────────────────────
#[test]
fn failed_resource_emits_blocking_diagnostic() {
    let g = graph(vec![resource_node(
        0,
        "file",
        "resource:linear",
        vec!["use-after-release"],
    )]);
    let report = ResourceChecker::check(&g);
    assert!(!report.diagnostics.is_empty());
    assert!(report.diagnostics[0].blocking);
}

// ── TRIANGULATE: claim format includes resource kind ─────────────────────
#[test]
fn entry_claim_includes_resource_kind() {
    let g = graph(vec![resource_node(
        0,
        "tx",
        "resource:affine",
        vec!["released"],
    )]);
    let report = ResourceChecker::check(&g);
    assert!(report.entries[0].claim.contains("resource:affine"));
}

// ── TASK-17: ResourceChecker structural edge analysis ────────────────────

#[test]
fn linear_resource_with_no_lifecycle_tag_and_no_edge_is_unverified() {
    // GIVEN a resource:linear node with no released tag AND no Consumes/Releases edge
    // THEN state is Unverified
    let g = graph(vec![resource_node(0, "lock", "resource:linear", vec![])]);
    let report = ResourceChecker::check(&g);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].state, VerificationState::Unverified);
    assert!(
        report.entries[0].evidence.as_deref().unwrap_or("").contains("E_RESOURCE_NO_LIFECYCLE_EDGE"),
        "evidence must reference E_RESOURCE_NO_LIFECYCLE_EDGE"
    );
}

#[test]
fn linear_resource_with_consumes_edge_is_proven() {
    // GIVEN a resource:linear node with an outgoing Consumes edge (no released tag)
    // THEN state is Proven (graph edge proves lifecycle)
    let node = resource_node(0, "file_handle", "resource:linear", vec![]);
    let consumer = GraphNode::new(NodeRef(1), NodeKind::Function, "fn.close");
    let g = SemanticGraph {
        nodes: vec![node, consumer],
        edges: vec![GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Consumes)],
    };
    let report = ResourceChecker::check(&g);
    let entry = report.entries.iter().find(|e| e.scope == "file_handle");
    assert!(entry.is_some(), "must have entry for file_handle");
    assert_eq!(
        entry.unwrap().state,
        VerificationState::Proven,
        "linear resource with Consumes edge must be Proven"
    );
}

#[test]
fn affine_resource_with_transaction_committed_is_proven() {
    // Existing behavior: affine + transaction-committed → Proven (unchanged)
    let g = graph(vec![resource_node(
        0,
        "tx",
        "resource:affine",
        vec!["transaction-committed"],
    )]);
    let report = ResourceChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Proven);
}

#[test]
fn shared_resource_with_safe_capability_edge_is_proven() {
    // GIVEN a resource:shared node with a SafeCapability edge (no safe-capability tag)
    // THEN state is Proven
    let node = resource_node(0, "shared_cache", "resource:shared", vec![]);
    let cap = GraphNode::new(NodeRef(1), NodeKind::Capability, "cap.safe_access");
    let g = SemanticGraph {
        nodes: vec![node, cap],
        edges: vec![GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::SafeCapability)],
    };
    let report = ResourceChecker::check(&g);
    let entry = report.entries.iter().find(|e| e.scope == "shared_cache");
    assert!(entry.is_some());
    assert_eq!(
        entry.unwrap().state,
        VerificationState::Proven,
        "shared resource with SafeCapability edge must be Proven"
    );
}
