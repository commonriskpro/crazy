// ── ail-verify::effect_checker tests ─────────────────────────────────────
//
// Integration tests for EffectChecker (verification pipeline step 8).
//
// EffectChecker rules:
//   R1: Inferred effects NOT in declared → Failed (E_EFFECT_UNDECLARED)
//   R2: Declared effects but no inferred → Assumed (E_EFFECT_UNUSED)
//   R3: Declared covers inferred → Proven
//   R4: Neither declared nor inferred → Proven

use ail_core::semantic_graph::{
    EdgeKind, EffectRow, GraphEdge, GraphNode, NodeKind, NodeRef, SemanticGraph,
};
use ail_verify::effect_checker::EffectChecker;
use ail_verify::report::VerificationState;

fn node(id: u32, kind: NodeKind, name: &str) -> GraphNode {
    GraphNode::new(NodeRef(id), kind, name)
}

fn emits_edge(source: u32, target: u32) -> GraphEdge {
    GraphEdge::new(NodeRef(source), NodeRef(target), EdgeKind::Emits)
}

// ── Scenario R4: Pure node (no effects) → Proven ─────────────────────────
// GIVEN a Function node with no EffectRow and no Emits edges
// WHEN EffectChecker::check is called
// THEN entry state is Proven

#[test]
fn pure_node_no_effects_is_proven() {
    let graph = SemanticGraph {
        nodes: vec![node(0, NodeKind::Function, "pure_fn")],
        edges: vec![],
    };
    let report = EffectChecker::check(&graph);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].state, VerificationState::Proven);
    assert_eq!(report.entries[0].scope, "pure_fn");
}

// ── Scenario R3: Declared covers inferred → Proven ───────────────────────
// GIVEN a Function node with EffectRow { effects: ["IO"] }
// AND an Emits edge from that function to a node named "IO"
// WHEN EffectChecker::check is called
// THEN entry state is Proven

#[test]
fn declared_covers_inferred_is_proven() {
    let mut fn_node = node(0, NodeKind::Function, "io_fn");
    fn_node.effect_row = Some(EffectRow {
        effects: vec!["IO".into()],
    });
    let io_node = node(1, NodeKind::Effect, "IO");
    let graph = SemanticGraph {
        nodes: vec![fn_node, io_node],
        edges: vec![emits_edge(0, 1)],
    };
    let report = EffectChecker::check(&graph);
    // fn_node entry should be Proven; io_node entry should also be Proven (pure)
    let fn_entry = report.entries.iter().find(|e| e.scope == "io_fn").unwrap();
    assert_eq!(fn_entry.state, VerificationState::Proven);
}

// ── Scenario R1: Undeclared inferred effect → Failed (E_EFFECT_UNDECLARED) ──
// GIVEN a Function node with NO EffectRow
// AND an Emits edge from that function to "Database"
// WHEN EffectChecker::check is called
// THEN entry state is Failed with E_EFFECT_UNDECLARED in evidence

#[test]
fn undeclared_inferred_effect_is_failed() {
    let fn_node = node(0, NodeKind::Function, "db_fn");
    let db_node = node(1, NodeKind::Effect, "Database");
    let graph = SemanticGraph {
        nodes: vec![fn_node, db_node],
        edges: vec![emits_edge(0, 1)],
    };
    let report = EffectChecker::check(&graph);
    let fn_entry = report.entries.iter().find(|e| e.scope == "db_fn").unwrap();
    assert_eq!(fn_entry.state, VerificationState::Failed);
    let evidence = fn_entry.evidence.as_deref().unwrap_or("");
    assert!(
        evidence.contains("E_EFFECT_UNDECLARED"),
        "evidence must contain E_EFFECT_UNDECLARED, got: {evidence}"
    );
    assert!(
        evidence.contains("Database"),
        "evidence must name the undeclared effect"
    );
}

// ── Scenario R2: Declared but no inferred → Assumed (E_EFFECT_UNUSED) ─────
// GIVEN a Function node with EffectRow { effects: ["FileSystem"] }
// AND no Emits edges
// WHEN EffectChecker::check is called
// THEN entry state is Assumed with E_EFFECT_UNUSED in evidence

#[test]
fn declared_but_unused_effect_is_assumed() {
    let mut fn_node = node(0, NodeKind::Function, "unused_fn");
    fn_node.effect_row = Some(EffectRow {
        effects: vec!["FileSystem".into()],
    });
    let graph = SemanticGraph {
        nodes: vec![fn_node],
        edges: vec![],
    };
    let report = EffectChecker::check(&graph);
    assert_eq!(report.entries.len(), 1);
    let entry = &report.entries[0];
    assert_eq!(entry.state, VerificationState::Assumed);
    let evidence = entry.evidence.as_deref().unwrap_or("");
    assert!(
        evidence.contains("E_EFFECT_UNUSED"),
        "evidence must contain E_EFFECT_UNUSED, got: {evidence}"
    );
}

// ── Scenario R1 override: partial undeclared beats partial coverage ────────
// GIVEN declared: ["IO"] but inferred: ["IO", "Network"]
// THEN entry is Failed (Network is undeclared)

#[test]
fn partial_undeclared_is_failed() {
    let mut fn_node = node(0, NodeKind::Function, "partial_fn");
    fn_node.effect_row = Some(EffectRow {
        effects: vec!["IO".into()],
    });
    let io_node = node(1, NodeKind::Effect, "IO");
    let net_node = node(2, NodeKind::Effect, "Network");
    let graph = SemanticGraph {
        nodes: vec![fn_node, io_node, net_node],
        edges: vec![emits_edge(0, 1), emits_edge(0, 2)],
    };
    let report = EffectChecker::check(&graph);
    let fn_entry = report
        .entries
        .iter()
        .find(|e| e.scope == "partial_fn")
        .unwrap();
    assert_eq!(fn_entry.state, VerificationState::Failed);
    let evidence = fn_entry.evidence.as_deref().unwrap_or("");
    assert!(evidence.contains("E_EFFECT_UNDECLARED"));
    assert!(evidence.contains("Network"));
}

// ── Triangulation: empty graph → empty report ────────────────────────────

#[test]
fn empty_graph_produces_empty_report() {
    let graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };
    let report = EffectChecker::check(&graph);
    assert_eq!(report.entries.len(), 0);
    assert_eq!(report.summary(), VerificationState::Proven);
}

// ── Triangulation: Calls edges are ignored (not Emits) ───────────────────

#[test]
fn calls_edges_are_not_inferred_effects() {
    let caller = node(0, NodeKind::Function, "caller");
    let callee = node(1, NodeKind::Function, "callee");
    let graph = SemanticGraph {
        nodes: vec![caller, callee],
        edges: vec![GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls)], // NOT Emits — should be ignored
    };
    let report = EffectChecker::check(&graph);
    // caller: no declared, no inferred Emits → Proven
    let caller_entry = report.entries.iter().find(|e| e.scope == "caller").unwrap();
    assert_eq!(
        caller_entry.state,
        VerificationState::Proven,
        "Calls edges must not be counted as inferred effects"
    );
}

// ── Triangulation: report carries schema_version and counts ───────────────

#[test]
fn report_enrichment_is_set() {
    let fn_node = node(0, NodeKind::Function, "fn");
    let graph = SemanticGraph {
        nodes: vec![fn_node],
        edges: vec![],
    };
    let report = EffectChecker::check(&graph);
    assert_eq!(report.schema_version, "verification/1.0");
    assert_eq!(report.summary_counts.verified_count, 1);
}
