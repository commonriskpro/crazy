pub(super) use ail_core::semantic_graph::EdgeKind;
pub(super) use ail_core::semantic_graph::{
    AssociatedTypeBinding, ConstraintSet, ContractClauses, EffectRow, GenericParamDecl,
    GenericParamKind, GraphEdge, GraphNode, HandlerMeta, InferredFact, InterfaceImplMeta, NodeKind,
    NodeRef, ParamDecl, RefinementRef, RefinementStatus, RuntimeCheckMeta, SemanticGraph,
    TypeArgBinding, TypeFacts, WhereConstraint,
};
pub(super) use ail_verify::report::VerificationState;
pub(super) use ail_verify::type_checker::{E_MISSING_HASH, E_MISSING_ORD, TypeChecker};

pub(super) fn graph_from(nodes: Vec<GraphNode>) -> SemanticGraph {
    SemanticGraph {
        nodes,
        edges: vec![],
    }
}

pub(super) fn graph_with_edges(nodes: Vec<GraphNode>, edges: Vec<GraphEdge>) -> SemanticGraph {
    SemanticGraph { nodes, edges }
}

pub(super) fn type_node(id: u32, name: &str) -> GraphNode {
    GraphNode::new(NodeRef(id), NodeKind::Type, name)
}

pub(super) fn fn_node(id: u32, name: &str) -> GraphNode {
    GraphNode::new(NodeRef(id), NodeKind::Function, name)
}
