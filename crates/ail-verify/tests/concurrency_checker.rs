// ── ail-verify::concurrency_checker tests ────────────────────────────────
//
// Strict TDD — tests for concurrency safety verification.
// Spec: verification-pipeline/spec §2 (concurrency safety checker).

use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph, TrustMetadata};
use ail_verify::concurrency_checker::ConcurrencyChecker;
use ail_verify::report::VerificationState;

fn conc_node(id: u32, name: &str, level: &str, tags: Vec<&str>) -> GraphNode {
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

// ── Scenario: non-concurrency nodes are skipped ───────────────────────────
#[test]
fn non_concurrency_nodes_produce_no_entries() {
    let g = graph(vec![
        GraphNode::new(NodeRef(0), NodeKind::Function, "pure_fn"),
        GraphNode::new(NodeRef(1), NodeKind::Module, "core"),
    ]);
    let report = ConcurrencyChecker::check(&g);
    assert!(report.entries.is_empty());
}

// ── Scenario: orphan task → Failed ───────────────────────────────────────
// GIVEN a task node with "orphan" tag
// WHEN ConcurrencyChecker::check is called
// THEN state is Failed
#[test]
fn orphan_task_is_failed() {
    let g = graph(vec![conc_node(0, "bg_task", "task", vec!["orphan"])]);
    let report = ConcurrencyChecker::check(&g);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].state, VerificationState::Failed);
    assert!(
        report.entries[0]
            .evidence
            .as_ref()
            .unwrap()
            .contains("orphaned")
    );
}

// ── Scenario: awaited task → Proven ─────────────────────────────────────
// GIVEN a task node with "awaited" tag
// WHEN ConcurrencyChecker::check is called
// THEN state is Proven
#[test]
fn awaited_task_is_proven() {
    let g = graph(vec![conc_node(0, "fetch_task", "task", vec!["awaited"])]);
    let report = ConcurrencyChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Proven);
}

// ── Scenario: cancelled task → Proven ────────────────────────────────────
#[test]
fn cancelled_task_is_proven() {
    let g = graph(vec![conc_node(0, "timer_task", "task", vec!["cancelled"])]);
    let report = ConcurrencyChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Proven);
}

// ── Scenario: transferred task → Proven ──────────────────────────────────
#[test]
fn transferred_task_is_proven() {
    let g = graph(vec![conc_node(0, "work_task", "task", vec!["transferred"])]);
    let report = ConcurrencyChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Proven);
}

// ── Scenario: task with no lifecycle tag → Unverified ────────────────────
#[test]
fn task_with_no_lifecycle_tag_is_unverified() {
    let g = graph(vec![conc_node(0, "mystery_task", "task", vec![])]);
    let report = ConcurrencyChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Unverified);
}

// ── Scenario: closed task-group → Proven ─────────────────────────────────
// GIVEN a task-group node with "closed" tag
// WHEN ConcurrencyChecker::check is called
// THEN state is Proven
#[test]
fn closed_task_group_is_proven() {
    let g = graph(vec![conc_node(
        0,
        "worker_group",
        "task-group",
        vec!["closed"],
    )]);
    let report = ConcurrencyChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Proven);
}

// ── Scenario: unclosed task-group → Failed ───────────────────────────────
// GIVEN a task-group node WITHOUT "closed" tag
// WHEN ConcurrencyChecker::check is called
// THEN state is Failed
#[test]
fn unclosed_task_group_is_failed() {
    let g = graph(vec![conc_node(0, "leaky_group", "task-group", vec![])]);
    let report = ConcurrencyChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Failed);
}

// ── Scenario: closed channel → Proven ────────────────────────────────────
#[test]
fn closed_channel_is_proven() {
    let g = graph(vec![conc_node(0, "msg_chan", "channel", vec!["closed"])]);
    let report = ConcurrencyChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Proven);
}

// ── Scenario: channel with no close tag → Unverified ─────────────────────
#[test]
fn channel_without_close_is_unverified() {
    let g = graph(vec![conc_node(0, "open_chan", "channel", vec![])]);
    let report = ConcurrencyChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Unverified);
}

// ── Scenario: shared-state with safe-capability → Proven ─────────────────
#[test]
fn shared_state_with_safe_capability_is_proven() {
    let g = graph(vec![conc_node(
        0,
        "shared_map",
        "shared-state",
        vec!["safe-capability"],
    )]);
    let report = ConcurrencyChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Proven);
}

// ── Scenario: shared-state without safe-capability → Unsafe ──────────────
#[test]
fn shared_state_without_safe_capability_is_unsafe() {
    let g = graph(vec![conc_node(0, "mutable_global", "shared-state", vec![])]);
    let report = ConcurrencyChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Unsafe);
}

// ── Scenario: cell-crossing → Failed ─────────────────────────────────────
// GIVEN a cell-crossing node (Cell<T> crosses task boundary)
// WHEN ConcurrencyChecker::check is called
// THEN state is always Failed
#[test]
fn cell_crossing_is_always_failed() {
    let g = graph(vec![conc_node(
        0,
        "cell_ref",
        "cell-crossing",
        vec![], // no tags needed — always fails
    )]);
    let report = ConcurrencyChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Failed);
}

// ── Scenario: unbounded-concurrency with policy-approved → Assumed ─────────
#[test]
fn unbounded_concurrency_with_policy_approved_is_assumed() {
    let g = graph(vec![conc_node(
        0,
        "thread_pool",
        "unbounded-concurrency",
        vec!["policy-approved"],
    )]);
    let report = ConcurrencyChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Assumed);
}

// ── Scenario: unbounded-concurrency without approval → Unverified ─────────
#[test]
fn unbounded_concurrency_without_approval_is_unverified() {
    let g = graph(vec![conc_node(
        0,
        "thread_pool",
        "unbounded-concurrency",
        vec![],
    )]);
    let report = ConcurrencyChecker::check(&g);
    assert_eq!(report.entries[0].state, VerificationState::Unverified);
}

// ── TRIANGULATE: diagnostics emitted for Failed/Unsafe ────────────────────
#[test]
fn failed_concurrency_emits_blocking_diagnostic() {
    let g = graph(vec![conc_node(0, "orphan", "task", vec!["orphan"])]);
    let report = ConcurrencyChecker::check(&g);
    assert!(!report.diagnostics.is_empty());
    assert!(report.diagnostics[0].blocking);
}

// ── TRIANGULATE: summary reflects worst state ─────────────────────────────
#[test]
fn summary_reflects_worst_concurrency_state() {
    let g = graph(vec![
        conc_node(0, "ok_task", "task", vec!["awaited"]),
        conc_node(1, "cell_cross", "cell-crossing", vec![]),
    ]);
    let report = ConcurrencyChecker::check(&g);
    assert_eq!(report.summary(), VerificationState::Failed);
}
