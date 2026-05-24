use super::*;

// ── impact_query_returns_dependents ───────────────────────────────────
// Spec: Impact query returns nodes that depend on the target.
//
// Graph: A --DependsOn--> B --Calls--> C
// Impact(B) should return A (depends on B via reverse DependsOn edge).
#[test]
fn impact_query_returns_dependents() {
    // A=0, B=1, C=2.  0 --DependsOn--> 1, 1 --Calls--> 2
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Module, "A"),
            GraphNode::new(NodeRef(1), NodeKind::Function, "B"),
            GraphNode::new(NodeRef(2), NodeKind::Function, "C"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::DependsOn),
            GraphEdge::new(NodeRef(1), NodeRef(2), EdgeKind::Calls),
        ],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::Impact {
        target: NodeRef(1),
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions())
        .expect("impact build must succeed");
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    // A (0) depends on B (1) via DependsOn; should appear.
    assert!(ids.contains(&0), "A must appear in impact(B); got: {ids:?}");
    // B (1) is the target itself; should NOT appear.
    assert!(
        !ids.contains(&1),
        "target B must not be in its own impact set; got: {ids:?}"
    );
}

// ── impact_missing_target_returns_node_not_found ──────────────────────
#[test]
fn impact_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let query = ContextQuery::Impact {
        target: NodeRef(99),
        budget: QueryBudget::default(),
    };
    let result = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions());
    assert_eq!(result, Err(ContextError::NodeNotFound));
}

// ── callers_direct_returns_direct_callers ────────────────────────────
// Spec: Callers(transitive=false) returns only direct callers.
//
// Graph: A --Calls--> B --Calls--> C
// Callers(C, transitive=false) = {B} only.
#[test]
fn callers_direct_returns_direct_callers() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Function, "A"),
            GraphNode::new(NodeRef(1), NodeKind::Function, "B"),
            GraphNode::new(NodeRef(2), NodeKind::Function, "C"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls),
            GraphEdge::new(NodeRef(1), NodeRef(2), EdgeKind::Calls),
        ],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::Callers {
        target: NodeRef(2), // C
        transitive: false,
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions()).unwrap();
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert_eq!(ids, vec![1], "direct callers of C is only B; got: {ids:?}");
}

// ── callers_transitive_returns_all_callers ────────────────────────────
// TRIANGULATE: transitive=true must follow the call chain further back.
#[test]
fn callers_transitive_returns_all_callers() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Function, "A"),
            GraphNode::new(NodeRef(1), NodeKind::Function, "B"),
            GraphNode::new(NodeRef(2), NodeKind::Function, "C"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls),
            GraphEdge::new(NodeRef(1), NodeRef(2), EdgeKind::Calls),
        ],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::Callers {
        target: NodeRef(2), // C
        transitive: true,
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions()).unwrap();
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    // Both A (0) and B (1) are transitive callers of C.
    assert!(
        ids.contains(&0),
        "A must be a transitive caller of C; got: {ids:?}"
    );
    assert!(
        ids.contains(&1),
        "B must be a direct caller of C; got: {ids:?}"
    );
    assert!(!ids.contains(&2), "C itself must not appear; got: {ids:?}");
}

// ── callers_missing_target_returns_node_not_found ────────────────────
#[test]
fn callers_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::Callers {
            target: NodeRef(99),
            transitive: false,
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    );
    assert_eq!(result, Err(ContextError::NodeNotFound));
}

// ── callees_direct_returns_direct_callees ─────────────────────────────
// Spec: Callees(transitive=false) returns only direct callees.
//
// Graph: A --Calls--> B --Calls--> C
// Callees(A, transitive=false) = {B} only.
#[test]
fn callees_direct_returns_direct_callees() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Function, "A"),
            GraphNode::new(NodeRef(1), NodeKind::Function, "B"),
            GraphNode::new(NodeRef(2), NodeKind::Function, "C"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls),
            GraphEdge::new(NodeRef(1), NodeRef(2), EdgeKind::Calls),
        ],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::Callees {
        target: NodeRef(0), // A
        transitive: false,
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions()).unwrap();
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert_eq!(ids, vec![1], "direct callees of A is only B; got: {ids:?}");
}

// ── callees_transitive_returns_all_callees ────────────────────────────
// TRIANGULATE: transitive=true follows the call chain forward.
#[test]
fn callees_transitive_returns_all_callees() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Function, "A"),
            GraphNode::new(NodeRef(1), NodeKind::Function, "B"),
            GraphNode::new(NodeRef(2), NodeKind::Function, "C"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls),
            GraphEdge::new(NodeRef(1), NodeRef(2), EdgeKind::Calls),
        ],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::Callees {
        target: NodeRef(0), // A
        transitive: true,
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions()).unwrap();
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert!(
        ids.contains(&1),
        "B must be a transitive callee of A; got: {ids:?}"
    );
    assert!(
        ids.contains(&2),
        "C must be a transitive callee of A; got: {ids:?}"
    );
    assert!(!ids.contains(&0), "A itself must not appear; got: {ids:?}");
}

// ── callees_missing_target_returns_node_not_found ────────────────────
#[test]
fn callees_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::Callees {
            target: NodeRef(99),
            transitive: false,
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    );
    assert_eq!(result, Err(ContextError::NodeNotFound));
}

// ── dyn_calls_included_in_callers_and_callees ─────────────────────────
// Spec: Callers/Callees queries must include DynCalls edges alongside
// static Calls edges, so that dynamic dispatch via `Dyn<Interface>` is
// visible in caller/callee results.
//
// Graph:
//   A --Calls-->    B  (static)
//   A --DynCalls--> C  (dynamic dispatch)
//
// Callees(A, transitive=false) = {B, C}
// Callers(B, transitive=false) = {A}
// Callers(C, transitive=false) = {A}
#[test]
fn dyn_calls_included_in_callers_and_callees() {
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Function, "A"),
            GraphNode::new(NodeRef(1), NodeKind::Function, "B"),
            GraphNode::new(NodeRef(2), NodeKind::Function, "C_dyn"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls),
            GraphEdge::new(NodeRef(0), NodeRef(2), EdgeKind::DynCalls),
        ],
    };
    let snapshot = make_snapshot();

    // Callees(A) must include both B (static) and C (dynamic).
    let callees_resp = ResponseBuilder::build(
        &ContextQuery::Callees {
            target: NodeRef(0),
            transitive: false,
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    )
    .expect("callees must succeed");
    let callee_ids: Vec<u32> = callees_resp.structured.iter().map(|n| n.id.0).collect();
    assert!(
        callee_ids.contains(&1),
        "static callee B must appear; got: {callee_ids:?}"
    );
    assert!(
        callee_ids.contains(&2),
        "dynamic callee C must appear; got: {callee_ids:?}"
    );

    // Callers(C) must include A (via DynCalls).
    let callers_resp = ResponseBuilder::build(
        &ContextQuery::Callers {
            target: NodeRef(2),
            transitive: false,
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    )
    .expect("callers must succeed");
    let caller_ids: Vec<u32> = callers_resp.structured.iter().map(|n| n.id.0).collect();
    assert!(
        caller_ids.contains(&0),
        "A must appear as dynamic caller of C; got: {caller_ids:?}"
    );
}

// ── effects_query_returns_target_and_emits ────────────────────────────
// Spec: Effects query returns target plus nodes reachable via Emits edges.
//
// make_graph(): 0 --DependsOn--> 1, 1 --Emits--> 2
// Effects(1) should return {1, 2}.
#[test]
fn effects_query_returns_target_and_emits() {
    let graph = make_graph(); // 0→1(DependsOn), 1→2(Emits)
    let snapshot = make_snapshot();
    let query = ContextQuery::Effects {
        target: NodeRef(1),
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions()).unwrap();
    let ids: Vec<u32> = resp.structured.iter().map(|n| n.id.0).collect();
    assert!(
        ids.contains(&1),
        "target node 1 must be in effects; got: {ids:?}"
    );
    assert!(
        ids.contains(&2),
        "emitted node 2 must be in effects; got: {ids:?}"
    );
    assert!(
        !ids.contains(&0),
        "node 0 (DependsOn, not Emits) must not appear; got: {ids:?}"
    );
}

// ── effects_missing_target_returns_node_not_found ────────────────────
#[test]
fn effects_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::Effects {
            target: NodeRef(99),
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    );
    assert_eq!(result, Err(ContextError::NodeNotFound));
}

// ── contracts_query_returns_target_node ──────────────────────────────
// Spec: Contracts query returns only the target node.
#[test]
fn contracts_query_returns_target_node() {
    use ail_core::semantic_graph::ContractClauses;
    let mut target_node = GraphNode::new(NodeRef(1), NodeKind::Function, "pay");
    target_node.contract_clauses = Some(ContractClauses {
        requires: vec!["amount > 0".to_string()],
        ensures: vec!["balance_changed".to_string()],
    });
    let graph = SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Module, "billing"),
            target_node.clone(),
        ],
        edges: vec![],
    };
    let snapshot = make_snapshot();
    let query = ContextQuery::Contracts {
        target: NodeRef(1),
        budget: QueryBudget::default(),
    };
    let resp = ResponseBuilder::build(&query, &graph, &snapshot, &no_redactions()).unwrap();
    assert_eq!(
        resp.structured.len(),
        1,
        "contracts query must return exactly 1 node"
    );
    assert_eq!(resp.structured[0].id, NodeRef(1));
    assert!(
        resp.structured[0].contract_clauses.is_some(),
        "contract_clauses must be present on the returned node"
    );
}

// ── contracts_missing_target_returns_node_not_found ──────────────────
#[test]
fn contracts_missing_target_returns_node_not_found() {
    let graph = make_graph();
    let snapshot = make_snapshot();
    let result = ResponseBuilder::build(
        &ContextQuery::Contracts {
            target: NodeRef(99),
            budget: QueryBudget::default(),
        },
        &graph,
        &snapshot,
        &no_redactions(),
    );
    assert_eq!(result, Err(ContextError::NodeNotFound));
}
