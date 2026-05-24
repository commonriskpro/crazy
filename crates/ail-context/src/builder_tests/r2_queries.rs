use super::*;

// ── R2 TESTS ──────────────────────────────────────────────────────────
// All tests below cover the 10 new query variants + rich response fields.

// ── r2_diff_query_returns_all_nodes ───────────────────────────────────
// Spec: Diff query returns structural differences between snapshots.
// Without two materialised graphs, returns all nodes from current graph.
#[test]
fn r2_diff_query_returns_all_nodes() {
    let graph = make_graph(); // 3 nodes
    let snapshot = make_snapshot();
    let query = ContextQuery::Diff {
        snapshot_a: None,
        snapshot_b: None,
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("diff build must succeed");
    assert_eq!(
        resp.structured.len(),
        3,
        "diff query must return all nodes from current graph; got {:?}",
        resp.structured.len()
    );
}

// ── r2_diff_query_zero_budget_rejected ────────────────────────────────
#[test]
fn r2_diff_query_zero_budget_rejected() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::Diff {
            snapshot_a: None,
            snapshot_b: None,
            budget: QueryBudget::bytes(0),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    );
    assert_eq!(result, Err(ContextError::InvalidBudget));
}

// ── r2_risks_query_returns_target_and_breaks_if_changed ───────────────
// Spec: Risks query returns target + BreaksIfChanged-reachable nodes.
//
// Graph: A --BreaksIfChanged--> B, A --Calls--> C (Calls excluded)
// Risks(A) = {A, B}
#[test]
fn r2_risks_query_returns_target_and_breaks_if_changed() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Function, "A"),
            GraphNode::new(NodeRef(1), NodeKind::Type, "B"),
            GraphNode::new(NodeRef(2), NodeKind::Function, "C"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::BreaksIfChanged),
            GraphEdge::new(NodeRef(0), NodeRef(2), EdgeKind::Calls),
        ],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::Risks {
        target: NodeRef(0),
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("risks build must succeed");
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert!(ids.contains(&0), "target A must be in result; got: {ids:?}");
    assert!(
        ids.contains(&1),
        "B (BreaksIfChanged) must be in result; got: {ids:?}"
    );
    assert!(
        !ids.contains(&2),
        "C (Calls, not risk) must not appear; got: {ids:?}"
    );
}

// ── r2_risks_missing_target_returns_node_not_found ────────────────────
#[test]
fn r2_risks_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::Risks {
            target: NodeRef(99),
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    );
    assert_eq!(result, Err(ContextError::NodeNotFound));
}

// ── r2_todo_query_returns_target_and_proves_nodes ─────────────────────
// Spec: Todo query returns outstanding proof obligations.
//
// Graph: fn.checkout --Proves--> invariant.stock
// Todo(fn.checkout) = {fn.checkout, invariant.stock}
#[test]
fn r2_todo_query_returns_target_and_proves_nodes() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Function, "checkout"),
            GraphNode::new(NodeRef(1), NodeKind::Invariant, "stock"),
            GraphNode::new(NodeRef(2), NodeKind::Module, "unrelated"),
        ],
        edges: vec![GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Proves)],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::Todo {
        target: NodeRef(0),
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("todo build must succeed");
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert!(ids.contains(&0), "target must be in todo; got: {ids:?}");
    assert!(
        ids.contains(&1),
        "proves node must be in todo; got: {ids:?}"
    );
    assert!(!ids.contains(&2), "unrelated must not appear; got: {ids:?}");
}

// ── r2_todo_missing_target_returns_node_not_found ─────────────────────
#[test]
fn r2_todo_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::Todo {
            target: NodeRef(99),
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    );
    assert_eq!(result, Err(ContextError::NodeNotFound));
}

// ── r2_capabilities_query_returns_target_and_emits_depends ───────────
// Spec: Capabilities returns target + Emits + DependsOn reachable nodes.
//
// Graph: module --Emits--> cap.payment, module --DependsOn--> dep.db
//        module --Calls--> fn.pay (excluded)
// Capabilities(module) = {module, cap.payment, dep.db}
#[test]
fn r2_capabilities_query_returns_target_and_emits_depends() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Module, "checkout"),
            GraphNode::new(NodeRef(1), NodeKind::Capability, "payment"),
            GraphNode::new(NodeRef(2), NodeKind::Module, "db"),
            GraphNode::new(NodeRef(3), NodeKind::Function, "pay"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Emits),
            GraphEdge::new(NodeRef(0), NodeRef(2), EdgeKind::DependsOn),
            GraphEdge::new(NodeRef(0), NodeRef(3), EdgeKind::Calls), // excluded
        ],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::Capabilities {
        target: NodeRef(0),
        profile: "prod".to_string(),
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("capabilities build must succeed");
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert!(ids.contains(&0), "target must be in result; got: {ids:?}");
    assert!(ids.contains(&1), "Emits node must appear; got: {ids:?}");
    assert!(ids.contains(&2), "DependsOn node must appear; got: {ids:?}");
    assert!(
        !ids.contains(&3),
        "Calls node must not appear; got: {ids:?}"
    );
}

// ── r2_capabilities_missing_target_returns_node_not_found ────────────
#[test]
fn r2_capabilities_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::Capabilities {
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

// ── r2_handlers_query_returns_target_and_reverse_callers ─────────────
// Spec: Handlers returns target + nodes that call it (handler bindings).
//
// Graph: handler_A --Calls--> cap.payment, handler_B --Calls--> cap.payment
// Handlers(cap.payment) = {cap.payment, handler_A, handler_B}
#[test]
fn r2_handlers_query_returns_target_and_reverse_callers() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Capability, "payment"),
            GraphNode::new(NodeRef(1), NodeKind::Function, "handler_A"),
            GraphNode::new(NodeRef(2), NodeKind::Function, "handler_B"),
            GraphNode::new(NodeRef(3), NodeKind::Module, "unrelated"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(1), NodeRef(0), EdgeKind::Calls),
            GraphEdge::new(NodeRef(2), NodeRef(0), EdgeKind::Calls),
        ],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::Handlers {
        target: NodeRef(0),
        profile: "prod".to_string(),
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("handlers build must succeed");
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert!(
        ids.contains(&0),
        "target cap.payment must be in result; got: {ids:?}"
    );
    assert!(
        ids.contains(&1),
        "handler_A must be in result; got: {ids:?}"
    );
    assert!(
        ids.contains(&2),
        "handler_B must be in result; got: {ids:?}"
    );
    assert!(!ids.contains(&3), "unrelated must not appear; got: {ids:?}");
}

// ── r2_handlers_missing_target_returns_node_not_found ────────────────
#[test]
fn r2_handlers_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::Handlers {
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

// ── r2_concurrency_query_returns_reads_writes_calls ───────────────────
// Spec: Concurrency returns target + Reads/Writes/Calls reachable nodes.
//
// Graph: fn.process --Reads--> state, --Writes--> output, --Calls--> fn.sub
// Concurrency(fn.process) = {fn.process, state, output, fn.sub}
#[test]
fn r2_concurrency_query_returns_reads_writes_calls() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Function, "process"),
            GraphNode::new(NodeRef(1), NodeKind::Type, "state"),
            GraphNode::new(NodeRef(2), NodeKind::Type, "output"),
            GraphNode::new(NodeRef(3), NodeKind::Function, "sub"),
            GraphNode::new(NodeRef(4), NodeKind::Module, "unrelated"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Reads),
            GraphEdge::new(NodeRef(0), NodeRef(2), EdgeKind::Writes),
            GraphEdge::new(NodeRef(0), NodeRef(3), EdgeKind::Calls),
        ],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::Concurrency {
        target: NodeRef(0),
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("concurrency build must succeed");
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert!(ids.contains(&0), "target must be in result; got: {ids:?}");
    assert!(ids.contains(&1), "Reads node must appear; got: {ids:?}");
    assert!(ids.contains(&2), "Writes node must appear; got: {ids:?}");
    assert!(ids.contains(&3), "Calls node must appear; got: {ids:?}");
    assert!(!ids.contains(&4), "unrelated must not appear; got: {ids:?}");
}

// ── r2_concurrency_missing_target_returns_node_not_found ─────────────
#[test]
fn r2_concurrency_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::Concurrency {
            target: NodeRef(99),
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    );
    assert_eq!(result, Err(ContextError::NodeNotFound));
}

// ── r2_tasks_query_returns_calls_and_emits ────────────────────────────
// Spec: Tasks returns target + Calls/Emits reachable nodes (async tasks).
//
// Graph: fn.fetch --Calls--> fn.sub_task, --Emits--> effect.io
//        fn.fetch --Reads--> state (excluded — not Calls/Emits)
// Tasks(fn.fetch) = {fn.fetch, fn.sub_task, effect.io}
#[test]
fn r2_tasks_query_returns_calls_and_emits() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Function, "fetch"),
            GraphNode::new(NodeRef(1), NodeKind::Function, "sub_task"),
            GraphNode::new(NodeRef(2), NodeKind::Effect, "io"),
            GraphNode::new(NodeRef(3), NodeKind::Type, "state"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls),
            GraphEdge::new(NodeRef(0), NodeRef(2), EdgeKind::Emits),
            GraphEdge::new(NodeRef(0), NodeRef(3), EdgeKind::Reads),
        ],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::Tasks {
        target: NodeRef(0),
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("tasks build must succeed");
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert!(ids.contains(&0), "target must be in result; got: {ids:?}");
    assert!(ids.contains(&1), "Calls sub_task must appear; got: {ids:?}");
    assert!(ids.contains(&2), "Emits io must appear; got: {ids:?}");
    assert!(
        !ids.contains(&3),
        "Reads state (not Calls/Emits) must not appear; got: {ids:?}"
    );
}

// ── r2_tasks_missing_target_returns_node_not_found ────────────────────
#[test]
fn r2_tasks_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::Tasks {
            target: NodeRef(99),
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    );
    assert_eq!(result, Err(ContextError::NodeNotFound));
}

// ── r2_assumptions_query_returns_boundary_nodes ───────────────────────
// Spec: Assumptions returns trust assumption nodes (Boundary kind) reachable
// from target.
//
// Graph: module --DependsOn--> boundary.Stripe, --DependsOn--> fn.pay (not Boundary)
// Assumptions(module) = {module, boundary.Stripe}
#[test]
fn r2_assumptions_query_returns_boundary_nodes() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Module, "checkout"),
            GraphNode::new(NodeRef(1), NodeKind::Boundary, "Stripe"),
            GraphNode::new(NodeRef(2), NodeKind::Function, "pay"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::DependsOn),
            GraphEdge::new(NodeRef(0), NodeRef(2), EdgeKind::DependsOn),
        ],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::Assumptions {
        target: NodeRef(0),
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("assumptions build must succeed");
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert!(ids.contains(&0), "target must be in result; got: {ids:?}");
    assert!(
        ids.contains(&1),
        "boundary Stripe must appear; got: {ids:?}"
    );
    assert!(
        !ids.contains(&2),
        "non-boundary fn.pay must not appear; got: {ids:?}"
    );
}

// ── r2_assumptions_missing_target_returns_node_not_found ─────────────
#[test]
fn r2_assumptions_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::Assumptions {
            target: NodeRef(99),
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    );
    assert_eq!(result, Err(ContextError::NodeNotFound));
}

// ── r2_extract_candidates_returns_inner_nodes_without_external_callers ─
// Spec: ExtractCandidates returns sub-nodes of target with no external callers.
//
// Graph: target(0) --Calls--> inner(1), inner(1) has no external caller.
//        outer(2) --Calls--> inner(1) would make it non-candidate (excluded).
// ExtractCandidates(0) = {0, 1} — inner has only 0 as caller (within scope).
#[test]
fn r2_extract_candidates_no_external_callers() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Function, "target"),
            GraphNode::new(NodeRef(1), NodeKind::Function, "inner"),
            GraphNode::new(NodeRef(2), NodeKind::Module, "unrelated"),
        ],
        edges: vec![GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls)],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::ExtractCandidates {
        target: NodeRef(0),
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("extract_candidates build must succeed");
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert!(ids.contains(&0), "target must be in result; got: {ids:?}");
    assert!(
        ids.contains(&1),
        "inner (no external callers) must be a candidate; got: {ids:?}"
    );
    assert!(!ids.contains(&2), "unrelated must not appear; got: {ids:?}");
}

// ── r2_extract_candidates_excludes_externally_called_nodes ───────────
// TRIANGULATE: a node called by an external caller is excluded.
//
// Graph: target(0) --Calls--> inner(1), external(2) --Calls--> inner(1)
// ExtractCandidates(0) = {0} only — inner has external caller (2).
#[test]
fn r2_extract_candidates_excludes_externally_called_nodes() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Function, "target"),
            GraphNode::new(NodeRef(1), NodeKind::Function, "inner"),
            GraphNode::new(NodeRef(2), NodeKind::Module, "external"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls),
            GraphEdge::new(NodeRef(2), NodeRef(1), EdgeKind::Calls),
        ],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::ExtractCandidates {
        target: NodeRef(0),
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("extract_candidates build must succeed");
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert!(ids.contains(&0), "target must be in result; got: {ids:?}");
    assert!(
        !ids.contains(&1),
        "inner with external caller must NOT be a candidate; got: {ids:?}"
    );
}

// ── r2_extract_candidates_missing_target_returns_node_not_found ───────
#[test]
fn r2_extract_candidates_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::ExtractCandidates {
            target: NodeRef(99),
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    );
    assert_eq!(result, Err(ContextError::NodeNotFound));
}

// ── r2_move_safety_returns_target_destination_callers_contracts_effects ─
// Spec: MoveSafety returns target + destination + callers + contracts + effects.
//
// Graph: caller(0) --Calls--> target(1), target(1) --Proves--> contract(2),
//        target(1) --Emits--> effect(3), destination(4) exists.
// MoveSafety(target=1, dest=4) = {0,1,2,3,4}
#[test]
fn r2_move_safety_returns_all_affected_nodes() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Function, "caller"),
            GraphNode::new(NodeRef(1), NodeKind::Function, "target_fn"),
            GraphNode::new(NodeRef(2), NodeKind::Invariant, "contract"),
            GraphNode::new(NodeRef(3), NodeKind::Effect, "effect"),
            GraphNode::new(NodeRef(4), NodeKind::Module, "destination"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls),
            GraphEdge::new(NodeRef(1), NodeRef(2), EdgeKind::Proves),
            GraphEdge::new(NodeRef(1), NodeRef(3), EdgeKind::Emits),
        ],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::MoveSafety {
        target: NodeRef(1),
        destination: NodeRef(4),
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("move_safety build must succeed");
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert!(ids.contains(&0), "caller must be in result; got: {ids:?}");
    assert!(ids.contains(&1), "target must be in result; got: {ids:?}");
    assert!(ids.contains(&2), "contract must be in result; got: {ids:?}");
    assert!(ids.contains(&3), "effect must be in result; got: {ids:?}");
    assert!(
        ids.contains(&4),
        "destination must be in result; got: {ids:?}"
    );
}

// ── r2_move_safety_missing_target_returns_node_not_found ─────────────
#[test]
fn r2_move_safety_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::MoveSafety {
            target: NodeRef(99),
            destination: NodeRef(0),
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    );
    assert_eq!(result, Err(ContextError::NodeNotFound));
}
