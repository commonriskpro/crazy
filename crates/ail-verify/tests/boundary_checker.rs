// ── ail-verify::boundary_checker tests ───────────────────────────────────
//
// Strict TDD — tests for boundary/FFI trust verification.
// Spec: verification-pipeline/spec §3 (boundary/FFI trust checker).

use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph, TrustLevel, TrustMetadata};
use ail_verify::boundary_checker::BoundaryChecker;
use ail_verify::report::VerificationState;

fn boundary_node(id: u32, name: &str, tags: Vec<&str>) -> GraphNode {
    let mut node = GraphNode::new(NodeRef(id), NodeKind::Boundary, name);
    node.trust_metadata = Some(TrustMetadata {
        level: TrustLevel::Custom("boundary".to_string()),
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

/// All required tags for a fully-declared boundary.
fn all_required_tags() -> Vec<&'static str> {
    vec![
        "has-trust-level",
        "has-contract",
        "has-handler",
        "has-owner",
        "has-review-policy",
    ]
}

// ── Scenario: non-boundary nodes are skipped ─────────────────────────────
#[test]
fn non_boundary_nodes_are_skipped() {
    let g = graph(vec![
        GraphNode::new(NodeRef(0), NodeKind::Function, "fn_a"),
        GraphNode::new(NodeRef(1), NodeKind::Module, "mod_b"),
    ]);
    let report = BoundaryChecker::check(&g);
    assert!(report.entries.is_empty());
}

// ── Scenario: fully-declared boundary → Assumed ───────────────────────────
// GIVEN a boundary node with all required tags present
// WHEN BoundaryChecker::check is called
// THEN state is Assumed (boundaries are assumed by nature)
#[test]
fn fully_declared_boundary_is_assumed() {
    let g = graph(vec![boundary_node(0, "stripe", all_required_tags())]);
    let report = BoundaryChecker::check(&g);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].state, VerificationState::Assumed);
    assert!(report.entries[0].evidence.is_none());
}

// ── Scenario: expired assumption → Failed ────────────────────────────────
// GIVEN a boundary node with "expired-assumption" tag
// WHEN BoundaryChecker::check is called
// THEN state is Failed
#[test]
fn expired_assumption_is_failed() {
    let mut tags = all_required_tags();
    tags.push("expired-assumption");
    let g = graph(vec![boundary_node(0, "old_api", tags)]);
    let report = BoundaryChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Failed);
    assert!(
        report.entries[0]
            .evidence
            .as_ref()
            .unwrap()
            .contains("expired")
    );
}

// ── Scenario: no-contract boundary → Failed ──────────────────────────────
// GIVEN a boundary node with "no-contract" tag
// WHEN BoundaryChecker::check is called
// THEN state is Failed
#[test]
fn no_contract_boundary_is_failed() {
    let g = graph(vec![boundary_node(0, "raw_api", vec!["no-contract"])]);
    let report = BoundaryChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Failed);
}

// ── Scenario: unsafe-ffi without approval → Unsafe ───────────────────────
// GIVEN a boundary node with "unsafe-ffi" tag but no "approved" tag
// WHEN BoundaryChecker::check is called
// THEN state is Unsafe
#[test]
fn unsafe_ffi_without_approval_is_unsafe() {
    let g = graph(vec![boundary_node(0, "raw_ffi", vec!["unsafe-ffi"])]);
    let report = BoundaryChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Unsafe);
    assert!(
        report.entries[0]
            .evidence
            .as_ref()
            .unwrap()
            .contains("unchecked FFI")
    );
}

// ── Scenario: unsafe-ffi with approval → check required tags ─────────────
// GIVEN a boundary node with "unsafe-ffi" AND "approved" tags (but missing other required)
// WHEN BoundaryChecker::check is called
// THEN Unsafe check passes, falls through to required-tag check → Unverified
#[test]
fn unsafe_ffi_with_approval_falls_through_to_required_tag_check() {
    let g = graph(vec![boundary_node(
        0,
        "approved_ffi",
        vec!["unsafe-ffi", "approved"], // no other required tags
    )]);
    let report = BoundaryChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Unverified);
}

// ── Scenario: unsafe-ffi + approved + all required tags → Assumed ─────────
#[test]
fn unsafe_ffi_approved_with_all_required_tags_is_assumed() {
    let mut tags = all_required_tags();
    tags.push("unsafe-ffi");
    tags.push("approved");
    let g = graph(vec![boundary_node(0, "approved_unsafe", tags)]);
    let report = BoundaryChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Assumed);
}

// ── Scenario: boundary without trust_metadata → Unverified ───────────────
// GIVEN a NodeKind::Boundary node with NO trust_metadata at all
// WHEN BoundaryChecker::check is called
// THEN state is Unverified
#[test]
fn boundary_without_trust_metadata_is_unverified() {
    let node = GraphNode::new(NodeRef(0), NodeKind::Boundary, "bare_boundary");
    // no trust_metadata set
    let g = graph(vec![node]);
    let report = BoundaryChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Unverified);
}

// ── Scenario: boundary with some missing required tags → Unverified ───────
// GIVEN a boundary node with only "has-trust-level" and "has-contract" (missing 3)
// WHEN BoundaryChecker::check is called
// THEN state is Unverified with evidence listing missing tags
#[test]
fn boundary_with_missing_required_tags_is_unverified() {
    let g = graph(vec![boundary_node(
        0,
        "partial_stripe",
        vec!["has-trust-level", "has-contract"], // missing: has-handler, has-owner, has-review-policy
    )]);
    let report = BoundaryChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Unverified);
    let ev = report.entries[0].evidence.as_ref().unwrap();
    assert!(ev.contains("missing"));
}

// ── TRIANGULATE: summary worst state across boundaries ────────────────────
#[test]
fn summary_reflects_worst_boundary_state() {
    let g = graph(vec![
        boundary_node(0, "good", all_required_tags()),
        boundary_node(1, "bad", vec!["expired-assumption"]),
    ]);
    let report = BoundaryChecker::check(&g);
    assert_eq!(report.summary(), VerificationState::Failed);
}

// ── TRIANGULATE: multiple boundary nodes, each gets an entry ──────────────
#[test]
fn multiple_boundaries_each_get_entry() {
    let g = graph(vec![
        boundary_node(0, "stripe", all_required_tags()),
        boundary_node(1, "openai", vec!["has-trust-level", "has-contract"]),
        boundary_node(2, "raw_ffi", vec!["unsafe-ffi"]),
    ]);
    let report = BoundaryChecker::check(&g);
    assert_eq!(report.entries.len(), 3);
    assert_eq!(report.entries[0].state, VerificationState::Assumed);
    assert_eq!(report.entries[1].state, VerificationState::Unverified);
    assert_eq!(report.entries[2].state, VerificationState::Unsafe);
}

// ── TRIANGULATE: entry scope matches node name ────────────────────────────
#[test]
fn entry_scope_matches_node_name() {
    let g = graph(vec![boundary_node(0, "Stripe", all_required_tags())]);
    let report = BoundaryChecker::check(&g);
    assert_eq!(report.entries[0].scope, "Stripe");
}

// ── TASK-21: BoundaryChecker assumption lifecycle ─────────────────────────

#[test]
fn boundary_with_has_assumption_expired_tag_is_failed() {
    let g = graph(vec![boundary_node(0, "legacy_api", vec!["has-assumption-expired"])]);
    let report = BoundaryChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Failed);
    assert!(
        report.entries[0].evidence.as_deref().unwrap_or("").contains("E_BOUNDARY_ASSUMPTION_REVOKED"),
        "evidence must contain E_BOUNDARY_ASSUMPTION_REVOKED"
    );
}

#[test]
fn boundary_with_has_assumption_revoked_tag_is_failed() {
    let g = graph(vec![boundary_node(0, "old_ffi", vec!["has-assumption-revoked"])]);
    let report = BoundaryChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Failed);
    assert!(
        report.entries[0].evidence.as_deref().unwrap_or("").contains("E_BOUNDARY_ASSUMPTION_REVOKED")
    );
}

#[test]
fn boundary_with_has_assumption_proposed_only_is_unverified() {
    // proposed but not approved/active → Unverified
    let g = graph(vec![boundary_node(0, "pending_api", vec!["has-assumption-proposed"])]);
    let report = BoundaryChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Unverified);
}

#[test]
fn boundary_with_has_assumption_active_and_required_tags_is_assumed() {
    // has-assumption-active + all required tags → Assumed (existing flow)
    let mut tags = all_required_tags();
    tags.push("has-assumption-active");
    let g = graph(vec![boundary_node(0, "active_api", tags)]);
    let report = BoundaryChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Assumed);
}

#[test]
fn boundary_with_has_assumption_approved_and_required_tags_is_assumed() {
    // has-assumption-approved + all required tags → Assumed
    let mut tags = all_required_tags();
    tags.push("has-assumption-approved");
    let g = graph(vec![boundary_node(0, "approved_api", tags)]);
    let report = BoundaryChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Assumed);
}
