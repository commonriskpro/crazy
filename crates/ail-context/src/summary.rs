// ── ail-context::summary ──────────────────────────────────────────────────
//
// Deterministic summary renderer.
//
// # Policy
//
// Summary is always rendered from `structured` AFTER redaction and
// truncation have been applied.  The renderer has no access to withheld
// nodes, which prevents accidental leakage of redacted facts.

use ail_core::semantic_graph::GraphNode;

// ── render_summary ────────────────────────────────────────────────────────

/// Render a deterministic human-readable summary from the structured slice.
///
/// Each node produces one line of the form `"{Kind:?}: {name}"`.
/// Lines are joined with `'\n'`.  An empty slice returns an empty string.
///
/// The output is deterministic for a given `nodes` slice: the same input
/// always produces the same output.
pub fn render_summary(nodes: &[GraphNode]) -> String {
    if nodes.is_empty() {
        return String::new();
    }
    nodes
        .iter()
        .map(|n| format!("{:?}: {}", n.kind, n.name))
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef};

    // ── empty_structured_produces_empty_string ────────────────────────────
    // Spec: summary is rendered from structured only; empty slice → empty string.
    //
    // RED: `render_summary` did not exist → compile error.
    // GREEN: function signature + early return makes it compile and pass.
    #[test]
    fn empty_structured_produces_empty_string() {
        assert_eq!(render_summary(&[]), "");
    }

    // ── single_node_summary_contains_kind_and_name ───────────────────────
    // Spec: each node produces "{Kind}: {name}" in the summary.
    #[test]
    fn single_node_summary_contains_kind_and_name() {
        let node = GraphNode::new(NodeRef(0), NodeKind::Module, "core");
        let summary = render_summary(&[node]);
        assert!(
            summary.contains("core"),
            "summary must contain the node name; got: {summary}"
        );
        assert!(
            summary.contains("Module"),
            "summary must contain the node kind; got: {summary}"
        );
    }

    // ── TRIANGULATE: multiple_nodes_are_newline_separated ────────────────
    // Different from single-node: forces multi-node path and newline separator.
    #[test]
    fn multiple_nodes_are_newline_separated() {
        let nodes = vec![
            GraphNode::new(NodeRef(0), NodeKind::Module, "core"),
            GraphNode::new(NodeRef(1), NodeKind::Function, "run"),
        ];
        let summary = render_summary(&nodes);
        assert!(
            summary.contains('\n'),
            "summary for multiple nodes must contain a newline; got: {summary}"
        );
        assert!(
            summary.contains("core"),
            "summary must include first node name; got: {summary}"
        );
        assert!(
            summary.contains("run"),
            "summary must include second node name; got: {summary}"
        );
    }

    // ── TRIANGULATE: summary_is_deterministic_for_identical_input ────────
    // Same slice → same output (no randomness, no timestamps, no ordering drift).
    #[test]
    fn summary_is_deterministic_for_identical_input() {
        let nodes = vec![
            GraphNode::new(NodeRef(0), NodeKind::Effect, "io"),
            GraphNode::new(NodeRef(1), NodeKind::Capability, "net"),
        ];
        let a = render_summary(&nodes);
        let b = render_summary(&nodes);
        assert_eq!(a, b, "render_summary must be deterministic for identical input");
    }
}
