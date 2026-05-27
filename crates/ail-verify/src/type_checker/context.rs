use std::collections::BTreeMap;

use ail_core::semantic_graph::{GraphNode, NodeRef, SemanticGraph};

// ── TypeContext ────────────────────────────────────────────────────────────

/// Collected type facts indexed for efficient lookup.
///
/// Populated in a single pass over the graph.  All lookups are deterministic
/// (`BTreeMap` keys).
pub(crate) struct TypeContext<'a> {
    /// Nodes indexed by `NodeRef`.
    pub(crate) by_ref: BTreeMap<NodeRef, &'a GraphNode>,
    /// Nodes indexed by name.
    pub(crate) by_name: BTreeMap<&'a str, NodeRef>,
}

impl<'a> TypeContext<'a> {
    pub(crate) fn collect(graph: &'a SemanticGraph) -> Self {
        let mut by_ref = BTreeMap::new();
        let mut by_name = BTreeMap::new();
        for node in &graph.nodes {
            by_ref.insert(node.id, node);
            by_name.insert(node.name.as_str(), node.id);
        }
        TypeContext { by_ref, by_name }
    }

    pub(crate) fn get_by_name(&self, name: &str) -> Option<&GraphNode> {
        self.by_name
            .get(name)
            .and_then(|id| self.by_ref.get(id))
            .copied()
    }
}