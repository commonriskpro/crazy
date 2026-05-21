// ── ail-core::semantic_graph ──────────────────────────────────────────────
//
// Canonical typed graph representation for the AIL program model.
//
// # Identity contract
//
// `NodeRef(u32)` is the intra-graph identity for nodes within one
// `SemanticGraph`.  It is NOT a storage identity; that role belongs to
// `ail_storage::object::ObjectId`.  A `NodeRef` must never cross the storage
// boundary.
//
// # Determinism contract
//
// All serializable fields use `Vec` or `BTreeMap` — never `HashMap` — to
// guarantee CBOR output determinism with `ciborium`.  Validation helpers may
// build transient `BTreeSet` / `BTreeMap` structures internally, but those
// collections are never part of the serialized layout.

use serde::{Deserialize, Serialize};

// ── NodeRef ───────────────────────────────────────────────────────────────

/// Opaque intra-graph identity for a `GraphNode`.
///
/// Scoped to one `SemanticGraph`; must not be used as a storage key.
/// Implements `Ord`/`PartialOrd` so that validation logic can use
/// ordered sets without requiring `HashMap`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NodeRef(pub u32);

// ── NodeKind ──────────────────────────────────────────────────────────────

/// The semantic category of a `GraphNode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    /// A named module boundary.
    Module,
    /// A callable function.
    Function,
    /// A named type definition.
    Type,
    /// An effect declaration.
    Effect,
    /// A capability declaration.
    Capability,
    /// A contract definition.
    Contract,
    /// An invariant assertion.
    Invariant,
    /// A test node.
    Test,
    /// An architectural boundary marker.
    Boundary,
}

// ── EdgeKind ──────────────────────────────────────────────────────────────

/// The semantic relationship expressed by a `GraphEdge`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeKind {
    /// Caller → callee dependency.
    Calls,
    /// Reader → data dependency.
    Reads,
    /// Writer → data dependency.
    Writes,
    /// Emitter → effect dependency.
    Emits,
    /// General module-level dependency.
    DependsOn,
    /// Proof obligation edge.
    Proves,
    /// Change-impact edge.
    BreaksIfChanged,
}

// ── GraphNode ─────────────────────────────────────────────────────────────

/// A typed node in the semantic graph.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphNode {
    /// Intra-graph identity.
    pub id: NodeRef,
    /// Semantic category of this node.
    pub kind: NodeKind,
    /// Human-readable name (e.g., fully qualified symbol name).
    pub name: String,
}

// ── GraphEdge ─────────────────────────────────────────────────────────────

/// A directed, typed edge between two `GraphNode`s.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdge {
    /// Source node.
    pub source: NodeRef,
    /// Target node.
    pub target: NodeRef,
    /// Semantic relationship.
    pub kind: EdgeKind,
}

// ── SemanticGraph ─────────────────────────────────────────────────────────

/// The canonical program representation as a typed directed graph.
///
/// Uses `Vec` for both `nodes` and `edges` to guarantee deterministic CBOR
/// serialization.  Validation logic builds transient ordered sets internally.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticGraph {
    /// All nodes in the graph, in insertion order.
    pub nodes: Vec<GraphNode>,
    /// All edges in the graph, in insertion order.
    pub edges: Vec<GraphEdge>,
}

// ── GraphValidationError ──────────────────────────────────────────────────

/// Errors produced by `SemanticGraph::validate`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphValidationError {
    /// Two nodes share the same `NodeRef`.
    DuplicateRef(NodeRef),
    /// An edge endpoint references a `NodeRef` not present in the graph.
    DanglingEdge {
        /// The missing `NodeRef`.
        r#ref: NodeRef,
        /// Whether the missing ref was the edge source or target.
        role: DanglingRole,
    },
}

/// Whether a dangling edge endpoint was the source or the target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DanglingRole {
    Source,
    Target,
}

// ── SemanticGraph::validate ───────────────────────────────────────────────

impl SemanticGraph {
    /// Validate structural invariants:
    ///
    /// 1. All `NodeRef`s in `nodes` are unique.
    /// 2. Every edge endpoint corresponds to an existing node in this graph.
    ///
    /// Returns `Ok(())` when all invariants hold; otherwise returns the first
    /// `GraphValidationError` found.
    pub fn validate(&self) -> Result<(), GraphValidationError> {
        use std::collections::BTreeSet;

        // Pass 1 — build the set of known refs, detecting duplicates.
        let mut seen: BTreeSet<NodeRef> = BTreeSet::new();
        for node in &self.nodes {
            if !seen.insert(node.id) {
                return Err(GraphValidationError::DuplicateRef(node.id));
            }
        }

        // Pass 2 — verify all edge endpoints are in the known set.
        for edge in &self.edges {
            if !seen.contains(&edge.source) {
                return Err(GraphValidationError::DanglingEdge {
                    r#ref: edge.source,
                    role: DanglingRole::Source,
                });
            }
            if !seen.contains(&edge.target) {
                return Err(GraphValidationError::DanglingEdge {
                    r#ref: edge.target,
                    role: DanglingRole::Target,
                });
            }
        }

        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ───────────────────────────────────────────────────────────

    fn node(id: u32, kind: NodeKind, name: &str) -> GraphNode {
        GraphNode {
            id: NodeRef(id),
            kind,
            name: name.to_string(),
        }
    }

    fn edge(source: u32, target: u32, kind: EdgeKind) -> GraphEdge {
        GraphEdge {
            source: NodeRef(source),
            target: NodeRef(target),
            kind,
        }
    }

    // ── valid_graph_passes_validation ─────────────────────────────────────
    // Spec scenario: "Unique refs pass validation"
    //   GIVEN a graph with nodes NodeRef(0), NodeRef(1), NodeRef(2)
    //   WHEN validate() is called
    //   THEN validation returns Ok(())
    //
    // RED: written first — types exist now, validate() stubs returning Ok(())
    // GREEN: will pass with real implementation
    #[test]
    fn valid_graph_passes_validation() {
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Module, "core"),
                node(1, NodeKind::Function, "run"),
                node(2, NodeKind::Type, "Config"),
            ],
            edges: vec![edge(0, 1, EdgeKind::DependsOn), edge(1, 2, EdgeKind::Reads)],
        };
        assert_eq!(graph.validate(), Ok(()));
    }

    // ── duplicate_node_ref_is_rejected ────────────────────────────────────
    // Spec scenario: "Duplicate NodeRef is rejected"
    //   GIVEN a graph builder that inserts two nodes both with NodeRef(0)
    //   WHEN validate() is called
    //   THEN validation returns Err identifying the duplicate ref
    #[test]
    fn duplicate_node_ref_is_rejected() {
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Module, "a"),
                node(0, NodeKind::Function, "b"), // duplicate!
            ],
            edges: vec![],
        };
        assert_eq!(
            graph.validate(),
            Err(GraphValidationError::DuplicateRef(NodeRef(0)))
        );
    }

    // ── dangling_edge_source_is_rejected ──────────────────────────────────
    // Spec scenario: "Edge with missing source is rejected"
    //   GIVEN a graph containing NodeRef(1) but not NodeRef(99)
    //   WHEN an edge (NodeRef(99) → NodeRef(1)) is added and validate() called
    //   THEN validation returns Err naming the missing source ref
    #[test]
    fn dangling_edge_source_is_rejected() {
        let graph = SemanticGraph {
            nodes: vec![node(1, NodeKind::Function, "target_fn")],
            edges: vec![edge(99, 1, EdgeKind::Calls)], // source 99 is missing
        };
        assert_eq!(
            graph.validate(),
            Err(GraphValidationError::DanglingEdge {
                r#ref: NodeRef(99),
                role: DanglingRole::Source,
            })
        );
    }

    // ── dangling_edge_target_is_rejected ──────────────────────────────────
    // Spec scenario: "Edge with missing target"
    //   GIVEN a graph containing NodeRef(0) but not NodeRef(77)
    //   WHEN an edge (NodeRef(0) → NodeRef(77)) is added and validate() called
    //   THEN validation returns Err naming the missing target ref
    #[test]
    fn dangling_edge_target_is_rejected() {
        let graph = SemanticGraph {
            nodes: vec![node(0, NodeKind::Module, "source_mod")],
            edges: vec![edge(0, 77, EdgeKind::DependsOn)], // target 77 is missing
        };
        assert_eq!(
            graph.validate(),
            Err(GraphValidationError::DanglingEdge {
                r#ref: NodeRef(77),
                role: DanglingRole::Target,
            })
        );
    }

    // ── TRIANGULATE: edge_with_present_endpoints_passes ───────────────────
    // Spec scenario: "Edge with present endpoints passes"
    //   GIVEN a graph with NodeRef(0) and NodeRef(1)
    //   WHEN an edge (NodeRef(0) → NodeRef(1)) is added and validate() called
    //   THEN validation returns Ok(())
    //
    // Different from valid_graph_passes_validation: single edge, minimal setup.
    #[test]
    fn edge_with_present_endpoints_passes() {
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Module, "src"),
                node(1, NodeKind::Module, "dst"),
            ],
            edges: vec![edge(0, 1, EdgeKind::DependsOn)],
        };
        assert_eq!(graph.validate(), Ok(()));
    }

    // ── TRIANGULATE: empty_graph_passes_validation ────────────────────────
    // Edge case: a graph with no nodes and no edges is structurally valid.
    #[test]
    fn empty_graph_passes_validation() {
        let graph = SemanticGraph {
            nodes: vec![],
            edges: vec![],
        };
        assert_eq!(graph.validate(), Ok(()));
    }

    // ── cbor_encodes_deterministically ────────────────────────────────────
    // Spec scenario: "Re-serialization produces identical bytes"
    //   GIVEN a SemanticGraph serialized to CBOR
    //   WHEN the bytes are deserialized and re-serialized
    //   THEN the output bytes are identical to the original
    //
    // Uses ail_storage::codec::CborCodec — added as dev-dependency.
    #[test]
    fn cbor_encodes_deterministically() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Module, "mod_a"),
                node(1, NodeKind::Function, "fn_b"),
                node(2, NodeKind::Effect, "eff_c"),
            ],
            edges: vec![edge(0, 1, EdgeKind::DependsOn), edge(1, 2, EdgeKind::Emits)],
        };

        let bytes_a = codec.encode(&graph).expect("first encode must succeed");
        let bytes_b = codec.encode(&graph).expect("second encode must succeed");
        assert_eq!(
            bytes_a, bytes_b,
            "identical SemanticGraph inputs must produce identical CBOR bytes"
        );

        // TRIANGULATE: also verify re-deserialization produces the original.
        let decoded: SemanticGraph = codec.decode(&bytes_a).expect("decode must succeed");
        assert_eq!(
            decoded, graph,
            "decoded SemanticGraph must equal the original"
        );
    }

    // ── TRIANGULATE: different_graphs_produce_different_bytes ────────────
    // Forces non-trivial encoding: two distinct graphs must NOT hash the same.
    #[test]
    fn different_graphs_produce_different_bytes() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let graph_a = SemanticGraph {
            nodes: vec![node(0, NodeKind::Module, "a")],
            edges: vec![],
        };
        let graph_b = SemanticGraph {
            nodes: vec![node(0, NodeKind::Module, "b")], // different name
            edges: vec![],
        };

        let bytes_a = codec.encode(&graph_a).expect("encode a");
        let bytes_b = codec.encode(&graph_b).expect("encode b");
        assert_ne!(
            bytes_a, bytes_b,
            "graphs with different content must produce different CBOR bytes"
        );
    }
}
