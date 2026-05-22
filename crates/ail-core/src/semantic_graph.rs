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
    /// A package boundary in the semantic graph.
    ///
    /// Added in Phase 12 (packages-trust-model) as an additive variant.
    /// Existing CBOR fixtures that do not use `Package` are unaffected
    /// because `ciborium` encodes enum variants by name string.
    Package,
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

// ── Storage identity value types ─────────────────────────────────────────

/// A content-addressed hash identifying the binary object stored for a
/// `GraphNode`.
///
/// Carries the raw hex digest (e.g., a BLAKE3 hex string) as produced by the
/// storage layer.  The string is opaque to `ail-core`; storage is responsible
/// for producing and validating it.
///
/// Uses `String` rather than a fixed-size byte array so that the CBOR
/// encoding remains schema-version-agnostic and readable in debug output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentHash {
    /// Hex-encoded content digest (e.g., BLAKE3).
    pub hex: String,
}

/// The provenance of a `GraphNode` — which `ChangeSet` or operation created
/// or last modified it.
///
/// The value is an opaque identifier (e.g., `"change.add_checkout"`) as
/// defined by the storage and change-management layers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// Identifier of the originating change or operation.
    pub change_id: String,
}

/// The schema version under which a `GraphNode` was created.
///
/// Carries the schema name and version string (e.g., `"core_ir/2"`) so that
/// readers can apply the appropriate migrator if the current schema is newer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaRef {
    /// Schema identifier and version, e.g., `"core_ir/2"`.
    pub version: String,
}

/// Trust metadata associated with a `GraphNode`.
///
/// Records the trust level assigned by the policy layer and an optional
/// comment string.  The specific trust model (e.g., numeric level, role name)
/// is defined by the trust-management layer; `ail-core` treats the value as
/// opaque.
///
/// Uses `Vec<String>` for `tags` (not a `HashMap`) to guarantee deterministic
/// CBOR serialization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustMetadata {
    /// Trust level identifier (e.g., `"verified"`, `"unverified"`, `"high"`).
    pub level: String,
    /// Ordered tags qualifying the trust level (e.g., `["signed", "reviewed"]`).
    pub tags: Vec<String>,
}

// ── Semantic fact value types ─────────────────────────────────────────────

/// Contract clauses attached to a `GraphNode`.
///
/// Uses `Vec<String>` (no `HashMap`) for deterministic CBOR serialization.
/// Both lists are in declaration order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractClauses {
    /// Preconditions that callers must satisfy (e.g., `"x > 0"`).
    pub requires: Vec<String>,
    /// Postconditions the implementation guarantees (e.g., `"result >= 0"`).
    pub ensures: Vec<String>,
}

/// Materialized metadata for one runtime-check assertion on a `GraphNode`.
///
/// Does NOT execute anything; stores the predicate text and a stable content
/// hash so that tooling can track check identity across revisions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeCheckMeta {
    /// The predicate expression, as a string (e.g., `"x != null"`).
    pub predicate: String,
    /// A stable content hash identifying this predicate (e.g., a hex digest).
    pub hash: String,
}

/// Resolved type information for a `GraphNode`.
///
/// Uses only `Vec<String>` (no `HashMap`) to guarantee deterministic CBOR
/// serialization with `ciborium`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeFacts {
    /// The nominal type name (e.g., `"Int"`, `"Bool"`, `"Map"`).
    pub nominal: String,
    /// Type parameters, in declaration order (e.g., `["Key", "Value"]`).
    pub generics: Vec<String>,
}

/// Declared effect row for a `GraphNode`.
///
/// Uses `Vec<String>` (no `HashMap`) for CBOR determinism.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectRow {
    /// Named effects declared on this node (e.g., `["IO", "State"]`).
    pub effects: Vec<String>,
}

/// Declared capability requirements for a `GraphNode`.
///
/// Uses `Vec<String>` (no `HashMap`) for CBOR determinism.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityReqs {
    /// Named capability requirements (e.g., `["net:read", "fs:write"]`).
    pub caps: Vec<String>,
}

// ── GraphNode ─────────────────────────────────────────────────────────────

/// A typed node in the semantic graph.
///
/// # Backward Compatibility
///
/// All optional fields are serialized only when `Some` and deserialize as
/// `None` when absent.  This keeps every prior CBOR wire format byte-identical
/// when the corresponding fields are not populated.  Storage identity fields
/// (`content_hash`, `provenance`, `schema`, `trust_metadata`) were added in
/// G15 following the same pattern.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphNode {
    /// Intra-graph identity.
    pub id: NodeRef,
    /// Semantic category of this node.
    pub kind: NodeKind,
    /// Human-readable name (e.g., fully qualified symbol name).
    pub name: String,
    /// Resolved type information, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_facts: Option<TypeFacts>,
    /// Declared effect row, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_row: Option<EffectRow>,
    /// Declared capability requirements, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_reqs: Option<CapabilityReqs>,
    /// Contract clauses (requires/ensures), if declared.
    ///
    /// Serialized only when `Some`; absent fields deserialize as `None`.
    /// This keeps Phase 1–5 CBOR wire format byte-identical when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_clauses: Option<ContractClauses>,
    /// Materialized runtime-check metadata, if any checks are registered.
    ///
    /// Serialized only when `Some`; absent fields deserialize as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_checks: Option<Vec<RuntimeCheckMeta>>,
    /// Content-addressed hash of the stored binary object for this node.
    ///
    /// Populated by the storage layer after the node is persisted.
    /// `None` for in-memory nodes that have not yet been committed to storage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<ContentHash>,
    /// Provenance: which change or operation last created/modified this node.
    ///
    /// Set by the change-management layer when a `ChangeSet` is applied.
    /// `None` for nodes not yet associated with a committed change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
    /// Schema version under which this node was created.
    ///
    /// Used by the storage layer to select the appropriate migrator when
    /// reading older objects.  `None` for nodes created without explicit
    /// schema tracking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<SchemaRef>,
    /// Trust metadata assigned by the policy layer.
    ///
    /// `None` for nodes that have not yet passed through the trust-assignment
    /// pipeline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust_metadata: Option<TrustMetadata>,
}

impl GraphNode {
    /// Create a new `GraphNode` with all optional fields set to `None`.
    ///
    /// This is the preferred constructor for all phases and for new nodes that
    /// do not yet have resolved type/effect/capability or storage identity
    /// information.  Using this constructor avoids source-compat breaks when
    /// additional optional fields are added.
    ///
    /// Storage identity fields (`content_hash`, `provenance`, `schema`,
    /// `trust_metadata`) are also initialized to `None`; the storage and
    /// change-management layers populate them after persistence.
    ///
    /// # Example
    ///
    /// ```rust
    /// use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef};
    ///
    /// let node = GraphNode::new(NodeRef(0), NodeKind::Module, "core");
    /// assert_eq!(node.name, "core");
    /// assert!(node.type_facts.is_none());
    /// assert!(node.content_hash.is_none());
    /// ```
    pub fn new(id: NodeRef, kind: NodeKind, name: impl Into<String>) -> Self {
        Self {
            id,
            kind,
            name: name.into(),
            type_facts: None,
            effect_row: None,
            capability_reqs: None,
            contract_clauses: None,
            runtime_checks: None,
            content_hash: None,
            provenance: None,
            schema: None,
            trust_metadata: None,
        }
    }
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
        GraphNode::new(NodeRef(id), kind, name)
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

    // ── package_node_cbor_round_trip ──────────────────────────────────────
    // Spec scenario: "Package node round-trips through CBOR"
    //   GIVEN a GraphNode with kind: NodeKind::Package
    //   WHEN serialized to CBOR and deserialized
    //   THEN kind equals NodeKind::Package
    //
    // Also verifies the additive variant does not disturb existing node kinds.
    #[test]
    fn package_node_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Module, "root"),
                node(1, NodeKind::Package, "payments.stripe"),
            ],
            edges: vec![edge(0, 1, EdgeKind::DependsOn)],
        };

        let bytes = codec.encode(&graph).expect("encode must succeed");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode must succeed");

        assert_eq!(
            decoded, graph,
            "graph with Package node must survive CBOR round-trip"
        );
        assert_eq!(
            decoded.nodes[1].kind,
            NodeKind::Package,
            "Package kind must be preserved"
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
