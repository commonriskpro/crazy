use super::*;

// ── history_query_returns_target_and_chain ───────────────────────────
// Spec: History query returns the target node + provenance chain oldest-first.
//
// Chain: genesis (no parent) → snap2 (parent=genesis) → snap3 (parent=snap2)
// History(node, current=snap3) should return [genesis, snap2, snap3].
#[test]
fn history_query_returns_target_and_chain() {
    let genesis_id = ObjectId::from_bytes(b"genesis");
    let snap2_id = ObjectId::from_bytes(b"snap2");
    let snap3_id = ObjectId::from_bytes(b"snap3");

    let genesis = SnapshotEnvelope {
        id: genesis_id,
        graph_root_hash: genesis_id,
        parent_id: None,
        applied_change_id: None,
        created_at: 1_000,
        verification_report_hash: None,
        ..Default::default()
    };
    let snap2 = SnapshotEnvelope {
        id: snap2_id,
        graph_root_hash: snap2_id,
        parent_id: Some(genesis_id),
        applied_change_id: None,
        created_at: 2_000,
        verification_report_hash: None,
        ..Default::default()
    };
    let snap3 = SnapshotEnvelope {
        id: snap3_id,
        graph_root_hash: snap3_id,
        parent_id: Some(snap2_id),
        applied_change_id: None,
        created_at: 3_000,
        verification_report_hash: None,
        ..Default::default()
    };

    let graph = SemanticGraph {
        nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Function, "checkout")],
        edges: vec![],
    };
    let query = ContextQuery::History {
        target: NodeRef(0),
        budget: QueryBudget::default(),
    };
    let all_snapshots = vec![genesis.clone(), snap2.clone()];
    let resp = ResponseBuilder::build_with_history(
        &query,
        &graph,
        &snap3,
        &no_redactions(),
        &all_snapshots,
    )
    .expect("history build must succeed");

    // Structured must contain the target node.
    assert_eq!(resp.structured.len(), 1);
    assert_eq!(resp.structured[0].id, NodeRef(0));

    // History chain: oldest first.
    let chain_ids: Vec<u64> = resp.history_entries.iter().map(|s| s.created_at).collect();
    assert_eq!(
        chain_ids,
        vec![1_000, 2_000, 3_000],
        "history must be oldest-first; got: {chain_ids:?}"
    );
}

// ── history_query_single_snapshot ────────────────────────────────────
// TRIANGULATE: history with no parent yields a chain of length 1.
#[test]
fn history_query_single_snapshot() {
    let graph = SemanticGraph {
        nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Function, "fn0")],
        edges: vec![],
    };
    let snapshot = make_snapshot(); // no parent_id
    let query = ContextQuery::History {
        target: NodeRef(0),
        budget: QueryBudget::default(),
    };
    let resp =
        ResponseBuilder::build_with_history(&query, &graph, &snapshot, &no_redactions(), &[])
            .expect("history build must succeed");
    assert_eq!(
        resp.history_entries.len(),
        1,
        "single-snapshot history must have exactly 1 entry"
    );
}

// ── history_missing_target_returns_node_not_found ────────────────────
#[test]
fn history_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build_with_history(
        &ContextQuery::History {
            target: NodeRef(99),
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
        &[],
    );
    assert_eq!(result, Err(ContextError::NodeNotFound));
}

// ── proofs_query_returns_target_and_proves_nodes ──────────────────────
// Spec: Proofs query returns the target node plus Proves-edge reachable nodes.
//
// Graph: fn.checkout --Proves--> invariant.stock_never_negative
// Proofs(fn.checkout) = {fn.checkout, invariant.stock_never_negative}
//
// RED: ContextQuery::Proofs did not exist → compile error.
// GREEN: variant + builder arm makes it compile and pass.
#[test]
fn proofs_query_returns_target_and_proves_nodes() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Function, "checkout"),
            GraphNode::new(NodeRef(1), NodeKind::Invariant, "stock_never_negative"),
            GraphNode::new(NodeRef(2), NodeKind::Module, "unrelated"),
        ],
        edges: vec![GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Proves)],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::Proofs {
        target: NodeRef(0),
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("proofs build must succeed");
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert!(
        ids.contains(&0),
        "target checkout must be in proofs result; got: {ids:?}"
    );
    assert!(
        ids.contains(&1),
        "invariant stock_never_negative must be in proofs; got: {ids:?}"
    );
    assert!(
        !ids.contains(&2),
        "unrelated module must not appear; got: {ids:?}"
    );
}

// ── proofs_missing_target_returns_node_not_found ──────────────────────
#[test]
fn proofs_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::Proofs {
            target: NodeRef(99),
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    );
    assert_eq!(result, Err(ContextError::NodeNotFound));
}

// ── resources_query_returns_target_and_rw_nodes ───────────────────────
// Spec: Resources query returns target plus Reads/Writes-reachable nodes.
//
// Graph: fn.process_file --Reads--> file.handle, --Writes--> file.output
// Resources(fn.process_file) = {fn.process_file, file.handle, file.output}
//
// RED: ContextQuery::Resources did not exist → compile error.
// GREEN: variant + builder arm makes it compile and pass.
#[test]
fn resources_query_returns_target_and_rw_nodes() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Function, "process_file"),
            GraphNode::new(NodeRef(1), NodeKind::Type, "file.handle"),
            GraphNode::new(NodeRef(2), NodeKind::Type, "file.output"),
            GraphNode::new(NodeRef(3), NodeKind::Module, "unrelated"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Reads),
            GraphEdge::new(NodeRef(0), NodeRef(2), EdgeKind::Writes),
        ],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::Resources {
        target: NodeRef(0),
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("resources build must succeed");
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert!(ids.contains(&0), "target must be in result; got: {ids:?}");
    assert!(ids.contains(&1), "read dep must be in result; got: {ids:?}");
    assert!(
        ids.contains(&2),
        "write dep must be in result; got: {ids:?}"
    );
    assert!(!ids.contains(&3), "unrelated must not appear; got: {ids:?}");
}

// ── resources_missing_target_returns_node_not_found ───────────────────
#[test]
fn resources_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::Resources {
            target: NodeRef(99),
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    );
    assert_eq!(result, Err(ContextError::NodeNotFound));
}

// ── boundaries_query_returns_target_and_boundary_nodes ───────────────
// Spec: Boundaries query returns target plus Boundary-kind nodes reachable
// from it.
//
// Graph: module.checkout --DependsOn--> boundary.Stripe, --Calls--> fn.pay
// Boundaries(module.checkout) = {module.checkout, boundary.Stripe}
//                               (fn.pay is not a Boundary node)
//
// RED: ContextQuery::Boundaries did not exist → compile error.
// GREEN: variant + builder arm makes it compile and pass.
#[test]
fn boundaries_query_returns_target_and_boundary_nodes() {
    use ail_core::semantic_graph::NodeKind;
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Module, "checkout"),
            GraphNode::new(NodeRef(1), NodeKind::Boundary, "Stripe"),
            GraphNode::new(NodeRef(2), NodeKind::Function, "pay"), // not boundary
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::DependsOn),
            GraphEdge::new(NodeRef(0), NodeRef(2), EdgeKind::Calls),
        ],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::Boundaries {
        target: NodeRef(0),
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("boundaries build must succeed");
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert!(
        ids.contains(&0),
        "target module must be in result; got: {ids:?}"
    );
    assert!(
        ids.contains(&1),
        "Stripe boundary must be in result; got: {ids:?}"
    );
    assert!(
        !ids.contains(&2),
        "non-boundary fn.pay must not appear; got: {ids:?}"
    );
}

// ── boundaries_missing_target_returns_node_not_found ─────────────────
#[test]
fn boundaries_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::Boundaries {
            target: NodeRef(99),
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    );
    assert_eq!(result, Err(ContextError::NodeNotFound));
}

// ── why_query_returns_target_proves_breaks_and_history ───────────────
// Spec: Why query traces provenance via Proves and BreaksIfChanged edges
// and returns the snapshot history chain.
//
// Graph: fn.checkout --Proves--> invariant.paid, --BreaksIfChanged--> type.Cart
// Why(fn.checkout) = {fn.checkout, invariant.paid, type.Cart} + history
//
// RED: ContextQuery::Why did not exist → compile error.
// GREEN: variant + builder arm makes it compile and pass.
#[test]
fn why_query_returns_target_proves_breaks_and_history() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Function, "checkout"),
            GraphNode::new(NodeRef(1), NodeKind::Invariant, "paid"),
            GraphNode::new(NodeRef(2), NodeKind::Type, "Cart"),
            GraphNode::new(NodeRef(3), NodeKind::Module, "unrelated"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Proves),
            GraphEdge::new(NodeRef(0), NodeRef(2), EdgeKind::BreaksIfChanged),
        ],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::Why {
        target: NodeRef(0),
        budget: QueryBudget::default(),
    };
    let resp =
        ResponseBuilder::build_with_history(&query, &graph, &snapshot, &no_redactions(), &[])
            .expect("why build must succeed");
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert!(ids.contains(&0), "target must be in result; got: {ids:?}");
    assert!(
        ids.contains(&1),
        "invariant.paid (Proves) must be in result; got: {ids:?}"
    );
    assert!(
        ids.contains(&2),
        "type.Cart (BreaksIfChanged) must be in result; got: {ids:?}"
    );
    assert!(!ids.contains(&3), "unrelated must not appear; got: {ids:?}");
    // Why query also returns the history chain (even if 1 entry for genesis).
    assert_eq!(
        resp.history_entries.len(),
        1,
        "why query must carry history_entries; got: {:?}",
        resp.history_entries.len()
    );
}

// ── why_missing_target_returns_node_not_found ────────────────────────
#[test]
fn why_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::Why {
            target: NodeRef(99),
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    );
    assert_eq!(result, Err(ContextError::NodeNotFound));
}

// ── refactor_context_query_returns_callers_proves_effects ─────────────
// Spec: RefactorContext returns target + callers (to update) + proofs (to
// rerun) + effects (to preserve).
//
// Graph: A --Calls--> B --Proves--> C, B --Emits--> D
// RefactorContext(B) = {B, A(caller), C(proof), D(effect)}
//
// RED: ContextQuery::RefactorContext did not exist → compile error.
// GREEN: variant + builder arm makes it compile and pass.
#[test]
fn refactor_context_query_returns_callers_proves_effects() {
    // A=0, B=1, C=2, D=3
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Function, "A"),
            GraphNode::new(NodeRef(1), NodeKind::Function, "B"),
            GraphNode::new(NodeRef(2), NodeKind::Invariant, "C"),
            GraphNode::new(NodeRef(3), NodeKind::Effect, "D"),
            GraphNode::new(NodeRef(4), NodeKind::Module, "unrelated"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls), // A calls B → A is a caller
            GraphEdge::new(NodeRef(1), NodeRef(2), EdgeKind::Proves), // B proves C → C is a proof
            GraphEdge::new(NodeRef(1), NodeRef(3), EdgeKind::Emits), // B emits D → D is an effect
        ],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::RefactorContext {
        target: NodeRef(1), // B
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("refactor_context build must succeed");
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert!(ids.contains(&0), "caller A must be in result; got: {ids:?}");
    assert!(ids.contains(&1), "target B must be in result; got: {ids:?}");
    assert!(ids.contains(&2), "proof C must be in result; got: {ids:?}");
    assert!(ids.contains(&3), "effect D must be in result; got: {ids:?}");
    assert!(!ids.contains(&4), "unrelated must not appear; got: {ids:?}");
}

// ── refactor_context_missing_target_returns_node_not_found ───────────
#[test]
fn refactor_context_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::RefactorContext {
            target: NodeRef(99),
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    );
    assert_eq!(result, Err(ContextError::NodeNotFound));
}

// ── runtime_query_returns_target_and_emits_nodes ─────────────────────
// Spec: Runtime query returns target (with capability_reqs/effect_row) plus
// effect nodes reachable via Emits edges.
//
// Graph: fn.checkout --Emits--> effect.payment, fn.checkout --Calls--> fn.pay
// Runtime(fn.checkout) = {fn.checkout, effect.payment}
//                        (fn.pay not via Emits, excluded)
//
// RED: ContextQuery::Runtime did not exist → compile error.
// GREEN: variant + builder arm makes it compile and pass.
#[test]
fn runtime_query_returns_target_and_emits_nodes() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Function, "checkout"),
            GraphNode::new(NodeRef(1), NodeKind::Effect, "payment"),
            GraphNode::new(NodeRef(2), NodeKind::Function, "pay"), // Calls, not Emits
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Emits),
            GraphEdge::new(NodeRef(0), NodeRef(2), EdgeKind::Calls),
        ],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::Runtime {
        target: NodeRef(0),
        profile: "prod".to_string(),
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("runtime build must succeed");
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert!(
        ids.contains(&0),
        "target checkout must be in result; got: {ids:?}"
    );
    assert!(
        ids.contains(&1),
        "effect.payment (Emits) must be in result; got: {ids:?}"
    );
    assert!(
        !ids.contains(&2),
        "fn.pay (Calls, not Emits) must not appear; got: {ids:?}"
    );
}

// ── runtime_missing_target_returns_node_not_found ────────────────────
#[test]
fn runtime_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::Runtime {
            target: NodeRef(99),
            profile: "prod".to_string(),
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    );
    assert_eq!(result, Err(ContextError::NodeNotFound));
}

// ── g27_freshness_status_is_fresh_by_default ──────────────────────────
// Spec: ResponseBuilder always sets freshness_status = Fresh (the default).
//
// TRIANGULATE: forces the builder to set the field.
#[test]
fn g27_freshness_status_is_fresh_by_default() {
    use crate::dto::FreshnessStatus;
    let graph = make_graph();
    let snapshot = make_snapshot();
    let query = ContextQuery::Graph {
        scope: QueryScope::Full,
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("build must succeed");
    assert_eq!(
        resp.freshness_status,
        FreshnessStatus::Fresh,
        "builder must set freshness_status = Fresh"
    );
}
