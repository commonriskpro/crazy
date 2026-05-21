// ── ail-dogfood::stdlib_projection ───────────────────────────────────────
//
// Projects the v1 stdlib registry entries into a `SemanticGraph`, proving
// the stdlib's own module metadata can be represented as graph nodes.
//
// This module is part of PR 2 (changeset + stdlib + context integration).
// The public API surface is declared here; full implementation and tests
// ship with PR 2.

use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_stdlib::registry::StdlibRegistry;

/// Project all entries from a `StdlibRegistry` into a `SemanticGraph`.
///
/// Creates one `NodeKind::Module` node per registry entry, using the
/// entry's `name` field as the node name and `NodeRef(index as u32)` as
/// the intra-graph identity.
///
/// # Postconditions
///
/// - `result.nodes.len() == registry.entries.len()`
/// - Each node name equals the corresponding entry's `name` field
/// - All nodes have `NodeKind::Module`
/// - `result.validate() == Ok(())` (no edges, so no dangling refs)
pub fn project_stdlib_to_graph(registry: &StdlibRegistry) -> SemanticGraph {
    let nodes = registry
        .entries
        .iter()
        .enumerate()
        .map(|(i, entry)| GraphNode::new(NodeRef(i as u32), NodeKind::Module, entry.name.as_str()))
        .collect();

    SemanticGraph {
        nodes,
        edges: vec![],
    }
}
