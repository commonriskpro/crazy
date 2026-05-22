// ── Semantic graph fixture helpers ────────────────────────────────────────

/// Build a minimal but multi-typed [`ail_core::semantic_graph::SemanticGraph`]
/// fixture for use in workspace tests.
///
/// Returns a graph with:
/// - 3 nodes: `NodeRef(0)` (`Module`), `NodeRef(1)` (`Function`), `NodeRef(2)` (`Effect`)
/// - 2 edges: `0 → 1` (`DependsOn`), `1 → 2` (`Emits`)
///
/// The graph is structurally valid (`validate()` returns `Ok(())`).
///
/// # Example
///
/// ```rust
/// let graph = ail_testkit::make_semantic_graph();
/// assert!(graph.validate().is_ok());
/// ```
pub fn make_semantic_graph() -> ail_core::semantic_graph::SemanticGraph {
    use ail_core::semantic_graph::{
        EdgeKind, GraphEdge, GraphNode, NodeKind, NodeRef, SemanticGraph,
    };

    SemanticGraph {
        nodes: vec![
            GraphNode::new(NodeRef(0), NodeKind::Module, "core"),
            GraphNode::new(NodeRef(1), NodeKind::Function, "run"),
            GraphNode::new(NodeRef(2), NodeKind::Effect, "io"),
        ],
        edges: vec![
            GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::DependsOn),
            GraphEdge::new(NodeRef(1), NodeRef(2), EdgeKind::Emits),
        ],
    }
}

// ── Large graph fixture ───────────────────────────────────────────────────

/// Build a [`SemanticGraph`](ail_core::semantic_graph::SemanticGraph) with `n`
/// nodes connected by a linear `Calls` chain:
///
/// `NodeRef(0) → NodeRef(1) → NodeRef(2) → … → NodeRef(n-1)`
///
/// The resulting graph is structurally valid (`validate()` returns `Ok(())`).
/// Use this fixture for benchmarks and integration tests that need a realistic,
/// deterministic large graph without hand-crafting hundreds of nodes.
///
/// # Panics
///
/// Does not panic; `n = 0` returns an empty (valid) graph.
///
/// # Example
///
/// ```rust
/// let graph = ail_testkit::make_large_graph(500);
/// assert!(graph.validate().is_ok());
/// assert_eq!(graph.nodes.len(), 500);
/// ```
pub fn make_large_graph(n: usize) -> ail_core::semantic_graph::SemanticGraph {
    use ail_core::semantic_graph::{
        EdgeKind, GraphEdge, GraphNode, NodeKind, NodeRef, SemanticGraph,
    };

    let nodes: Vec<GraphNode> = (0..n)
        .map(|i| GraphNode::new(NodeRef(i as u32), NodeKind::Function, format!("fn_{i}")))
        .collect();

    // Linear chain: NodeRef(i) Calls NodeRef(i+1) for i in 0..n-1.
    let edges: Vec<GraphEdge> = (0..n.saturating_sub(1))
        .map(|i| GraphEdge::new(NodeRef(i as u32), NodeRef((i + 1) as u32), EdgeKind::Calls))
        .collect();

    SemanticGraph { nodes, edges }
}

// ── Storage fixture helpers ───────────────────────────────────────────────

/// Re-export of [`ail_storage::backends::memory::MemoryObjectStore`] for use
/// in tests across the workspace without an explicit `ail-storage` dependency.
pub use ail_storage::backends::memory::MemoryObjectStore;

/// Re-export of [`ail_storage::graph::ObjectBackedGraphStore`] for use in
/// workspace tests that need a `GraphStore` backed by an in-memory store.
pub use ail_storage::graph::ObjectBackedGraphStore;

/// Build a minimal [`ail_storage::graph::SnapshotEnvelope`] fixture.
///
/// `label` is hashed with BLAKE3 to produce both `id` and `graph_root_hash`,
/// giving a deterministic but unique `ObjectId` per call site.  `parent_id`
/// and `applied_change_id` are `None` (genesis snapshot) and `created_at`
/// is set to `0`.
///
/// # Example
///
/// ```rust
/// let snap = ail_testkit::make_snapshot_envelope("my-root");
/// assert!(snap.parent_id.is_none());
/// assert!(snap.applied_change_id.is_none());
/// ```
pub fn make_snapshot_envelope(label: &str) -> ail_storage::graph::SnapshotEnvelope {
    let id = ail_storage::object::ObjectId::from_bytes(label.as_bytes());
    ail_storage::graph::SnapshotEnvelope {
        id,
        graph_root_hash: id,
        parent_id: None,
        applied_change_id: None,
        created_at: 0,
        verification_report_hash: None,
    }
}

// ── Fixture path macro ────────────────────────────────────────────────────

/// Returns a [`std::path::PathBuf`] pointing to a file inside the **calling
/// crate's** `tests/fixtures/` directory.
///
/// Because this is a `macro_rules!` macro, `env!("CARGO_MANIFEST_DIR")` is
/// expanded at the call site, so the resolved path always belongs to the crate
/// that invokes the macro — not to `ail-testkit` itself.
///
/// # Panics
///
/// Panics with an informative message if the file does not exist at the
/// resolved path.
///
/// # Example
///
/// ```rust,no_run
/// // Inside a test in some other crate that depends on ail-testkit:
/// let path = ail_testkit::fixture!("sample.atl");
/// ```
#[macro_export]
macro_rules! fixture {
    ($name:expr) => {{
        let path = ::std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join($name);
        if !path.exists() {
            panic!(
                "fixture not found: {}\n\
                 Hint: create the file at that path to use it in tests.",
                path.display()
            );
        }
        path
    }};
}

#[cfg(test)]
mod tests {
    #[test]
    fn fixture_resolves_existing_file() {
        let path = crate::fixture!("sample.atl");
        assert!(path.exists(), "fixture path must exist");
    }

    #[test]
    #[should_panic(expected = "fixture not found")]
    fn fixture_panics_on_missing_file() {
        crate::fixture!("does_not_exist.atl");
    }

    // ── make_semantic_graph_is_valid ──────────────────────────────────────
    // Spec: make_semantic_graph() produces a structurally valid graph.
    //   GIVEN the fixture returned by make_semantic_graph()
    //   WHEN validate() is called on it
    //   THEN it returns Ok(())
    #[test]
    fn make_semantic_graph_is_valid() {
        let graph = crate::make_semantic_graph();
        assert!(
            graph.validate().is_ok(),
            "make_semantic_graph() fixture must pass structural validation"
        );
    }

    // ── Spec scenario: Large graph passes validation ───────────────────────
    // GIVEN make_large_graph(500) is called
    // WHEN graph.validate() is invoked
    // THEN validation returns Ok(())
    #[test]
    fn make_large_graph_500_is_valid() {
        let graph = crate::make_large_graph(500);
        assert_eq!(graph.nodes.len(), 500, "must have exactly 500 nodes");
        assert_eq!(graph.edges.len(), 499, "linear chain must have n-1 edges");
        assert!(
            graph.validate().is_ok(),
            "make_large_graph(500) must pass structural validation"
        );
    }

    // ── TRIANGULATE: make_large_graph(0) is valid ─────────────────────────
    #[test]
    fn make_large_graph_zero_is_valid() {
        let graph = crate::make_large_graph(0);
        assert_eq!(graph.nodes.len(), 0);
        assert_eq!(graph.edges.len(), 0);
        assert!(graph.validate().is_ok());
    }

    // ── TRIANGULATE: make_large_graph(1) has no edges ─────────────────────
    #[test]
    fn make_large_graph_one_has_no_edges() {
        let graph = crate::make_large_graph(1);
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.edges.len(), 0, "single node has no edges");
        assert!(graph.validate().is_ok());
    }
}
