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
//
// # Module layout
//
// - `types`      — all data type definitions (NodeKind, EdgeKind, GraphNode, …)
// - `validation` — GraphValidationError, DanglingRole, SemanticGraph::validate*
// - `refs`       — typed newtype wrappers (BlockRef, ContractRef, …)

mod refs;
mod types;
mod validation;

// ── Re-exports ────────────────────────────────────────────────────────────
//
// All items below were previously defined directly in this file.
// Re-exporting them here keeps every existing `ail_core::semantic_graph::Foo`
// path valid — downstream crates require zero changes.

pub use refs::{BlockRef, ContractRef, EffectRef, ProofObligationRef, RuntimeCheckRef};

pub use types::{
    Assertion, AssociatedTypeBinding, Binding, CapabilityArgBinding, CapabilityReqs, ConstraintSet,
    ContentHash, ContractClauses, EdgeKind, EffectArgBinding, EffectRow, GeneratedArtifact,
    GenericParamDecl, GenericParamKind, GraphEdge, GraphNode, HandlerMeta, InferredFact,
    InterfaceImplMeta, NodeKind, NodeRef, ParamDecl, Provenance, RefinementRef, RefinementStatus,
    RuntimeCheckMeta, SchemaRef, SemanticGraph, Span, TrustLevel, TrustMetadata, TypeArgBinding,
    TypeFacts, Visibility, WhereConstraint, WorkflowState,
};

pub use validation::{DanglingRole, GraphValidationError};

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ───────────────────────────────────────────────────────────

    fn node(id: u32, kind: NodeKind, name: &str) -> GraphNode {
        GraphNode::new(NodeRef(id), kind, name)
    }

    fn edge(source: u32, target: u32, kind: EdgeKind) -> GraphEdge {
        GraphEdge::new(NodeRef(source), NodeRef(target), kind)
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

    // ── G24: generic_params_cbor_round_trip ───────────────────────────────
    // Spec requirement 2 (Generics): GenericParamDecl with all kinds round-trips.
    //   GIVEN a Function node with generic_params covering all four kinds
    //   WHEN serialized to CBOR and deserialized
    //   THEN all fields are preserved exactly
    //
    // RED: written before GenericParamDecl / GenericParamKind existed.
    // GREEN: passes after Task 1 implementation.
    #[test]
    fn generic_params_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "traverse");
        node.generic_params = Some(vec![
            GenericParamDecl {
                name: "T".into(),
                kind: GenericParamKind::TypeParam,
                required_constraints: vec![],
            },
            GenericParamDecl {
                name: "e".into(),
                kind: GenericParamKind::EffectParam,
                required_constraints: vec![],
            },
            GenericParamDecl {
                name: "cap".into(),
                kind: GenericParamKind::CapabilityParam,
                required_constraints: vec![],
            },
            GenericParamDecl {
                name: "N".into(),
                kind: GenericParamKind::ConstParam,
                required_constraints: vec![],
            },
        ]);

        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode must succeed");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode must succeed");
        assert_eq!(
            decoded, graph,
            "graph with generic_params must survive CBOR round-trip"
        );
        assert_eq!(
            decoded.nodes[0].generic_params.as_ref().unwrap().len(),
            4,
            "all four generic param declarations must be preserved"
        );
    }

    // ── G24: params_and_return_type_cbor_round_trip ───────────────────────
    // Spec requirement 1 (Nominal): Function params with explicit types round-trip.
    //   GIVEN a Function node with declared params and return_type
    //   WHEN serialized to CBOR and deserialized
    //   THEN all param declarations are preserved
    #[test]
    fn params_and_return_type_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "load_user");
        node.params = Some(vec![ParamDecl {
            name: "id".into(),
            ty: "UserId".into(),
        }]);
        node.return_type = Some("User".into());

        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(decoded, graph);
        let params = decoded.nodes[0].params.as_ref().unwrap();
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "id");
        assert_eq!(params[0].ty, "UserId");
        assert_eq!(decoded.nodes[0].return_type.as_deref(), Some("User"));
    }

    // ── G24: interface_impl_meta_cbor_round_trip ──────────────────────────
    // Spec requirement 3 (Interfaces): InterfaceImplMeta with associated types.
    //   GIVEN a Type node with interface_impls including associated type bindings
    //   WHEN serialized to CBOR and deserialized
    //   THEN all impl data is preserved
    #[test]
    fn interface_impl_meta_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "PostgresUserRepo");
        node.interface_impls = Some(vec![InterfaceImplMeta {
            interface: "cap.Repository<User>".into(),
            associated_types: vec![AssociatedTypeBinding {
                name: "Error".into(),
                ty: "DbError".into(),
            }],
            is_adapter: false,
        }]);

        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(decoded, graph);
        let impls = decoded.nodes[0].interface_impls.as_ref().unwrap();
        assert_eq!(impls.len(), 1);
        assert_eq!(impls[0].interface, "cap.Repository<User>");
        assert_eq!(impls[0].associated_types[0].ty, "DbError");
        assert!(!impls[0].is_adapter);
    }

    // ── G24: refinement_ref_cbor_round_trip ──────────────────────────────
    // Spec requirement 6 (Refinements): RefinementRef with status and erasure flag.
    //   GIVEN a Type node with a refinement predicate and RuntimeChecked status
    //   WHEN serialized to CBOR and deserialized
    //   THEN all refinement fields are preserved
    #[test]
    fn refinement_ref_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "Email");
        node.refinement_ref = Some(RefinementRef {
            base_type: "Text".into(),
            predicate: "matches_email(value)".into(),
            status: RefinementStatus::RuntimeChecked,
            erased: false,
        });

        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(decoded, graph);
        let rf = decoded.nodes[0].refinement_ref.as_ref().unwrap();
        assert_eq!(rf.base_type, "Text");
        assert_eq!(rf.status, RefinementStatus::RuntimeChecked);
        assert!(!rf.erased);
    }

    // ── G24: constraint_set_cbor_round_trip ──────────────────────────────
    // Spec requirement 5 (Eq/Ord/Hash): ConstraintSet with all flags.
    //   GIVEN a Type node declaring Eq + Hash constraints
    //   WHEN serialized to CBOR and deserialized
    //   THEN constraint flags are preserved exactly
    #[test]
    fn constraint_set_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "UserId");
        node.constraint_set = Some(ConstraintSet {
            has_eq: true,
            has_ord: false,
            has_hash: true,
            has_partial_ord: false,
            extras: vec!["Display".into()],
        });

        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(decoded, graph);
        let cs = decoded.nodes[0].constraint_set.as_ref().unwrap();
        assert!(cs.has_eq);
        assert!(!cs.has_ord);
        assert!(cs.has_hash);
        assert_eq!(cs.extras, ["Display"]);
    }

    // ── G24: call_edge_with_args_cbor_round_trip ──────────────────────────
    // Spec requirement 1 (Nominal): Call edge with arg types and type bindings.
    //   GIVEN a Calls edge with call_args and type_arg_bindings
    //   WHEN serialized to CBOR and deserialized
    //   THEN all call-site metadata is preserved
    #[test]
    fn call_edge_with_args_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Function, "caller"),
                node(1, NodeKind::Function, "load_user"),
            ],
            edges: vec![GraphEdge {
                source: NodeRef(0),
                target: NodeRef(1),
                kind: EdgeKind::Calls,
                call_args: Some(vec!["OrderId".into()]),
                type_arg_bindings: Some(vec![TypeArgBinding {
                    param: "T".into(),
                    ty: "UserId".into(),
                }]),
                effect_arg_bindings: None,
                capability_arg_bindings: None,
            }],
        };

        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(decoded, graph);
        let e = &decoded.edges[0];
        assert_eq!(
            e.call_args.as_deref(),
            Some(["OrderId".to_string()].as_ref())
        );
        assert_eq!(e.type_arg_bindings.as_ref().unwrap()[0].ty, "UserId");
    }

    // ── G24: TRIANGULATE – node_without_new_fields_unchanged ─────────────
    // Backward compat: existing nodes without new fields round-trip unchanged.
    //   GIVEN a node created with GraphNode::new (all new fields None)
    //   WHEN serialized to CBOR and deserialized
    //   THEN all new optional fields remain None (not None → default junk)
    #[test]
    fn node_without_new_fields_unchanged() {
        use ail_storage::codec::{CborCodec, ContentCodec};

        let codec = CborCodec;
        let graph = SemanticGraph {
            nodes: vec![node(0, NodeKind::Function, "legacy_fn")],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        let n = &decoded.nodes[0];
        assert!(
            n.generic_params.is_none(),
            "generic_params must be None for legacy node"
        );
        assert!(n.params.is_none(), "params must be None for legacy node");
        assert!(
            n.return_type.is_none(),
            "return_type must be None for legacy node"
        );
        assert!(
            n.interface_impls.is_none(),
            "interface_impls must be None for legacy node"
        );
        assert!(
            n.refinement_ref.is_none(),
            "refinement_ref must be None for legacy node"
        );
        assert!(
            n.constraint_set.is_none(),
            "constraint_set must be None for legacy node"
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

    // ── G32: Semantic ref newtypes ────────────────────────────────────────

    // Spec: Each newtype wraps a String and is constructible.
    // RED: tests written before types existed; now GREEN after add.
    #[test]
    fn ref_newtypes_are_constructible() {
        let block = BlockRef("block_checkout".to_string());
        let contract = ContractRef("contract.payment".to_string());
        let effect = EffectRef("effect.db.read".to_string());
        let proof = ProofObligationRef("proof.invariant.balance".to_string());
        let rtcheck = RuntimeCheckRef("rtcheck.null_guard".to_string());

        assert_eq!(block.0, "block_checkout");
        assert_eq!(contract.0, "contract.payment");
        assert_eq!(effect.0, "effect.db.read");
        assert_eq!(proof.0, "proof.invariant.balance");
        assert_eq!(rtcheck.0, "rtcheck.null_guard");
    }

    // TRIANGULATE: two different values of the same newtype are not equal.
    #[test]
    fn ref_newtypes_inequality() {
        let a = BlockRef("block_a".to_string());
        let b = BlockRef("block_b".to_string());
        assert_ne!(
            a, b,
            "BlockRef with different inner values must not be equal"
        );

        let ca = ContractRef("c1".to_string());
        let cb = ContractRef("c2".to_string());
        assert_ne!(ca, cb);
    }

    // Spec: Ref newtypes are serde-transparent — CBOR encoding matches plain String.
    #[test]
    fn ref_newtype_cbor_is_transparent_with_string() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;

        let raw = "block_checkout_flow".to_string();
        let typed = BlockRef(raw.clone());

        let bytes_raw = codec.encode(&raw).expect("encode raw string");
        let bytes_typed = codec.encode(&typed).expect("encode BlockRef");

        assert_eq!(
            bytes_raw, bytes_typed,
            "BlockRef CBOR must be identical to plain String CBOR (transparent serde)"
        );
    }

    // TRIANGULATE: Ref newtype CBOR round-trip preserves value.
    #[test]
    fn ref_newtype_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;

        let original = ContractRef("contract.checkout.payment".to_string());
        let bytes = codec.encode(&original).expect("encode ContractRef");
        let decoded: ContractRef = codec.decode(&bytes).expect("decode ContractRef");
        assert_eq!(
            original, decoded,
            "ContractRef must survive CBOR round-trip"
        );
    }

    // ── Task C3 (RED): ConstraintSet::has_partial_ord ────────────────────

    // S-C3a: ConstraintSet with has_partial_ord=true round-trips through CBOR.
    #[test]
    fn constraint_set_with_has_partial_ord_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "Price");
        node.constraint_set = Some(ConstraintSet {
            has_eq: true,
            has_ord: false,
            has_hash: false,
            has_partial_ord: true,
            extras: vec![],
        });
        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        let cs = decoded.nodes[0]
            .constraint_set
            .as_ref()
            .expect("constraint_set must be Some");
        assert!(
            cs.has_partial_ord,
            "has_partial_ord must be true after round-trip"
        );
        assert!(!cs.has_ord, "has_ord must remain false");
    }

    // S-C3b: Old ConstraintSet without has_partial_ord deserializes with has_partial_ord=false.
    // Backward compatibility via serde default.
    #[test]
    fn legacy_constraint_set_has_partial_ord_defaults_false() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        // A legacy node with constraint_set that has no has_partial_ord field
        // in its CBOR bytes must deserialize with has_partial_ord=false.
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "Amount");
        node.constraint_set = Some(ConstraintSet {
            has_eq: true,
            has_ord: true,
            has_hash: false,
            has_partial_ord: false, // default — must not be emitted in CBOR when false
            extras: vec![],
        });
        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        let cs = decoded.nodes[0].constraint_set.as_ref().unwrap();
        assert!(!cs.has_partial_ord, "has_partial_ord must default to false");
        assert!(cs.has_ord, "has_ord must be preserved");
    }

    // ── Task D1 (RED): new NodeKind variants ──────────────────────────────

    // S-D1a: NodeKind::Interface, Impl, EffectAlias are constructible.
    #[test]
    fn new_node_kind_variants_are_constructible() {
        let _interface = NodeKind::Interface;
        let _impl_kind = NodeKind::Impl;
        let _effect_alias = NodeKind::EffectAlias;
        // All constructed without panic — test passes.
    }

    // S-D1b: Interface node CBOR round-trip preserves kind.
    #[test]
    fn interface_node_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        let graph = SemanticGraph {
            nodes: vec![GraphNode::new(
                NodeRef(0),
                NodeKind::Interface,
                "PaymentProvider",
            )],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(
            decoded.nodes[0].kind,
            NodeKind::Interface,
            "Interface kind must be preserved through CBOR round-trip"
        );
    }

    // S-D1c: Impl node round-trips and passes validation.
    // Triangulation: Impl is distinct from Interface in CBOR encoding.
    #[test]
    fn impl_node_round_trips_and_passes_validation() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Interface, "Chargeable"),
                GraphNode::new(NodeRef(1), NodeKind::Impl, "StripeChargeImpl"),
            ],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(decoded.nodes[0].kind, NodeKind::Interface);
        assert_eq!(decoded.nodes[1].kind, NodeKind::Impl);
        assert_eq!(
            decoded.validate(),
            Ok(()),
            "graph with Impl node must validate"
        );
    }

    // S-D1d: EffectAlias node round-trips.
    #[test]
    fn effect_alias_node_round_trips() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        let graph = SemanticGraph {
            nodes: vec![GraphNode::new(
                NodeRef(0),
                NodeKind::EffectAlias,
                "DatabaseAlias",
            )],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(decoded.nodes[0].kind, NodeKind::EffectAlias);
    }

    // ── Task D3 (RED): HandlerMeta on GraphNode ───────────────────────────

    // S-D3a: HandlerMeta with handled_caps is constructible and round-trips.
    #[test]
    fn handler_meta_with_caps_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "stripe_handler");
        node.handler_meta = Some(HandlerMeta {
            handled_caps: vec!["database.read".to_string(), "payments.charge".to_string()],
            internal_effects: vec!["IO".to_string()],
            satisfies_contract: Some("cap.payments.Chargeable".to_string()),
        });
        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        let hm = decoded.nodes[0]
            .handler_meta
            .as_ref()
            .expect("handler_meta must be Some");
        assert_eq!(hm.handled_caps, ["database.read", "payments.charge"]);
        assert_eq!(hm.internal_effects, ["IO"]);
        assert_eq!(
            hm.satisfies_contract.as_deref(),
            Some("cap.payments.Chargeable")
        );
    }

    // S-D3b: Old GraphNode without handler_meta deserializes with handler_meta=None.
    // Backward compatibility: existing fixtures must not break.
    #[test]
    fn legacy_node_without_handler_meta_has_none() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        let graph = SemanticGraph {
            nodes: vec![GraphNode::new(NodeRef(0), NodeKind::Function, "legacy_fn")],
            edges: vec![],
        };
        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert!(
            decoded.nodes[0].handler_meta.is_none(),
            "legacy node must have handler_meta=None after CBOR round-trip"
        );
    }

    // ── Task C1 (RED): EffectArgBinding and CapabilityArgBinding on GraphEdge ──
    // Tests written BEFORE the structs and fields exist — compilation fails = RED.

    // C1-1: EffectArgBinding is constructible and fields are correct.
    // Spec scenario: "EffectArgBinding CBOR round-trip"
    #[test]
    fn effect_arg_binding_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;

        let binding = EffectArgBinding {
            param: "e".to_string(),
            effects: vec!["IO".to_string()],
        };
        let bytes = codec.encode(&binding).expect("encode EffectArgBinding");
        let decoded: EffectArgBinding = codec.decode(&bytes).expect("decode EffectArgBinding");
        assert_eq!(decoded.param, "e");
        assert_eq!(decoded.effects, ["IO"]);
        assert_eq!(decoded, binding);
    }

    // C1-2: GraphEdge with effect_arg_bindings round-trips through CBOR.
    // Spec scenario: "EffectArgBinding CBOR round-trip" (on an edge)
    #[test]
    fn graph_edge_with_effect_arg_bindings_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;

        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Function, "caller"),
                node(1, NodeKind::Function, "callee"),
            ],
            edges: vec![GraphEdge {
                source: NodeRef(0),
                target: NodeRef(1),
                kind: EdgeKind::Calls,
                call_args: None,
                type_arg_bindings: None,
                effect_arg_bindings: Some(vec![EffectArgBinding {
                    param: "e".to_string(),
                    effects: vec!["IO".to_string()],
                }]),
                capability_arg_bindings: None,
            }],
        };

        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(decoded, graph);
        let bindings = decoded.edges[0]
            .effect_arg_bindings
            .as_ref()
            .expect("effect_arg_bindings must be Some");
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].param, "e");
        assert_eq!(bindings[0].effects, ["IO"]);
    }

    // C1-3: Edge without effect_arg_bindings is backward compatible (None after decode).
    // Spec scenario: "Edge without effect_arg_bindings is backward compatible"
    #[test]
    fn edge_without_effect_arg_bindings_is_backward_compat() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;

        // Simulate an edge encoded before EffectArgBinding field existed.
        // Creating it with the new constructor (None fields) produces identical
        // bytes to the old format (serde skips None).
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Function, "f"),
                node(1, NodeKind::Function, "g"),
            ],
            edges: vec![GraphEdge::new(NodeRef(0), NodeRef(1), EdgeKind::Calls)],
        };

        let bytes = codec.encode(&graph).expect("encode legacy edge");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert!(
            decoded.edges[0].effect_arg_bindings.is_none(),
            "legacy edge must decode with effect_arg_bindings=None"
        );
        assert!(
            decoded.edges[0].capability_arg_bindings.is_none(),
            "legacy edge must decode with capability_arg_bindings=None"
        );
    }

    // C1-4: CapabilityArgBinding is constructible and round-trips through CBOR.
    // Spec scenario: "CapabilityArgBinding" struct
    #[test]
    fn capability_arg_binding_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;

        let binding = CapabilityArgBinding {
            param: "cap".to_string(),
            caps: vec!["net:read".to_string()],
        };
        let bytes = codec.encode(&binding).expect("encode CapabilityArgBinding");
        let decoded: CapabilityArgBinding =
            codec.decode(&bytes).expect("decode CapabilityArgBinding");
        assert_eq!(decoded.param, "cap");
        assert_eq!(decoded.caps, ["net:read"]);
        assert_eq!(decoded, binding);
    }

    // C1-5 (TRIANGULATE): GraphEdge with both new fields round-trips.
    // Forces the real implementation to handle both fields simultaneously.
    #[test]
    fn graph_edge_with_both_arg_binding_fields_cbor_round_trip() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;

        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Function, "caller"),
                node(1, NodeKind::Function, "callee"),
            ],
            edges: vec![GraphEdge {
                source: NodeRef(0),
                target: NodeRef(1),
                kind: EdgeKind::Calls,
                call_args: None,
                type_arg_bindings: None,
                effect_arg_bindings: Some(vec![EffectArgBinding {
                    param: "e".to_string(),
                    effects: vec!["IO".to_string()],
                }]),
                capability_arg_bindings: Some(vec![CapabilityArgBinding {
                    param: "cap".to_string(),
                    caps: vec!["net:read".to_string()],
                }]),
            }],
        };

        let bytes = codec.encode(&graph).expect("encode");
        let decoded: SemanticGraph = codec.decode(&bytes).expect("decode");
        assert_eq!(decoded, graph);
        assert!(decoded.edges[0].effect_arg_bindings.is_some());
        assert!(decoded.edges[0].capability_arg_bindings.is_some());
    }

    // S-D3c: HandlerMeta without satisfies_contract omits that field.
    // Triangulation: None satisfies_contract must not appear in CBOR.
    #[test]
    fn handler_meta_without_contract_omits_field() {
        use ail_storage::codec::{CborCodec, ContentCodec};
        let codec = CborCodec;
        let mut node_with = GraphNode::new(NodeRef(0), NodeKind::Function, "h");
        node_with.handler_meta = Some(HandlerMeta {
            handled_caps: vec!["db.read".to_string()],
            internal_effects: vec![],
            satisfies_contract: Some("SomeContract".to_string()),
        });
        let mut node_without = GraphNode::new(NodeRef(0), NodeKind::Function, "h");
        node_without.handler_meta = Some(HandlerMeta {
            handled_caps: vec!["db.read".to_string()],
            internal_effects: vec![],
            satisfies_contract: None,
        });
        let bytes_with = codec
            .encode(&SemanticGraph {
                nodes: vec![node_with],
                edges: vec![],
            })
            .expect("encode with");
        let bytes_without = codec
            .encode(&SemanticGraph {
                nodes: vec![node_without],
                edges: vec![],
            })
            .expect("encode without");
        // Node with satisfies_contract must encode to MORE bytes.
        assert!(
            bytes_with.len() > bytes_without.len(),
            "satisfies_contract=Some must produce more bytes than None"
        );
    }

    // ── validate_full: valid graph returns empty errors ───────────────────
    // Spec: validate_full on a clean graph returns zero errors.
    //
    // RED: validate_full() did not exist → compile error.
    // GREEN: method added with all checks → returns empty vec.
    #[test]
    fn validate_full_valid_graph_returns_no_errors() {
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Module, "core"),
                node(1, NodeKind::Function, "run"),
                node(2, NodeKind::Effect, "io"),
            ],
            edges: vec![edge(0, 1, EdgeKind::DependsOn), edge(1, 2, EdgeKind::Emits)],
        };
        let errors = graph.validate_full();
        assert!(
            errors.is_empty(),
            "clean graph must produce zero errors; got: {errors:?}"
        );
    }

    // ── validate_full: duplicate ref detected ────────────────────────────
    // Spec: validate_full returns DuplicateRef for duplicate NodeRef(0).
    #[test]
    fn validate_full_detects_duplicate_ref() {
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Module, "a"),
                node(0, NodeKind::Function, "b"), // duplicate
            ],
            edges: vec![],
        };
        let errors = graph.validate_full();
        assert!(
            errors.contains(&GraphValidationError::DuplicateRef(NodeRef(0))),
            "must detect duplicate NodeRef(0); got: {errors:?}"
        );
    }

    // ── validate_full: dangling edge detected ─────────────────────────────
    // TRIANGULATE: different error kind from duplicate.
    // Spec: validate_full returns DanglingEdge for missing edge endpoint.
    #[test]
    fn validate_full_detects_dangling_edge() {
        let graph = SemanticGraph {
            nodes: vec![node(0, NodeKind::Module, "src")],
            edges: vec![edge(0, 99, EdgeKind::DependsOn)], // target 99 missing
        };
        let errors = graph.validate_full();
        assert!(
            errors.contains(&GraphValidationError::DanglingEdge {
                r#ref: NodeRef(99),
                role: DanglingRole::Target,
            }),
            "must detect dangling target NodeRef(99); got: {errors:?}"
        );
    }

    // ── validate_full: effect_row without Emits edge is rejected ─────────
    // Spec: A node with non-empty effect_row but no Emits edge is incoherent.
    //
    // RED: EffectRowNoEmitsEdge variant did not exist → compile error.
    // GREEN: Pass 3 in validate_full() detects the missing edge.
    #[test]
    fn validate_full_detects_effect_row_without_emits_edge() {
        let mut fn_node = node(0, NodeKind::Function, "pay");
        fn_node.effect_row = Some(EffectRow {
            effects: vec!["IO".to_string()],
        });
        let graph = SemanticGraph {
            nodes: vec![fn_node],
            edges: vec![], // no Emits edge!
        };
        let errors = graph.validate_full();
        assert!(
            errors.contains(&GraphValidationError::EffectRowNoEmitsEdge(NodeRef(0))),
            "must detect effect_row without Emits edge; got: {errors:?}"
        );
    }

    // ── validate_full: effect_row WITH Emits edge passes ─────────────────
    // TRIANGULATE: coherent effect_row must not produce an error.
    #[test]
    fn validate_full_effect_row_with_emits_edge_passes() {
        let mut fn_node = node(0, NodeKind::Function, "pay");
        fn_node.effect_row = Some(EffectRow {
            effects: vec!["IO".to_string()],
        });
        let io_node = node(1, NodeKind::Effect, "io");
        let graph = SemanticGraph {
            nodes: vec![fn_node, io_node],
            edges: vec![edge(0, 1, EdgeKind::Emits)],
        };
        let errors = graph.validate_full();
        let effect_row_errors: Vec<_> = errors
            .iter()
            .filter(|e| matches!(e, GraphValidationError::EffectRowNoEmitsEdge(_)))
            .collect();
        assert!(
            effect_row_errors.is_empty(),
            "coherent effect_row+Emits must not produce EffectRowNoEmitsEdge; got: {errors:?}"
        );
    }

    // ── validate_full: capability_reqs missing Capability node ───────────
    // Spec: A capability requirement that names a non-existent Capability node
    // is incoherent.
    //
    // RED: CapabilityReqsMissingNode variant did not exist → compile error.
    // GREEN: Pass 4 in validate_full() detects the missing node.
    #[test]
    fn validate_full_detects_capability_req_missing_node() {
        let mut fn_node = node(0, NodeKind::Function, "transfer");
        fn_node.capability_reqs = Some(CapabilityReqs {
            caps: vec!["net:read".to_string()],
        });
        let graph = SemanticGraph {
            nodes: vec![fn_node],
            edges: vec![], // no Capability node named "net:read"
        };
        let errors = graph.validate_full();
        assert!(
            errors.contains(&GraphValidationError::CapabilityReqsMissingNode {
                owner_ref: NodeRef(0),
                cap_name: "net:read".to_string(),
            }),
            "must detect missing Capability node 'net:read'; got: {errors:?}"
        );
    }

    // ── validate_full: capability_reqs WITH matching Capability node passes
    // TRIANGULATE: satisfied capability_reqs must not produce an error.
    #[test]
    fn validate_full_capability_reqs_with_matching_node_passes() {
        let mut fn_node = node(0, NodeKind::Function, "transfer");
        fn_node.capability_reqs = Some(CapabilityReqs {
            caps: vec!["net:read".to_string()],
        });
        let cap_node = node(1, NodeKind::Capability, "net:read");
        let graph = SemanticGraph {
            nodes: vec![fn_node, cap_node],
            edges: vec![],
        };
        let errors = graph.validate_full();
        let cap_errors: Vec<_> = errors
            .iter()
            .filter(|e| matches!(e, GraphValidationError::CapabilityReqsMissingNode { .. }))
            .collect();
        assert!(
            cap_errors.is_empty(),
            "satisfied capability_reqs must not produce errors; got: {errors:?}"
        );
    }

    // ── validate_full: multiple errors returned at once ───────────────────
    // Spec: validate_full returns ALL errors, not just the first one.
    #[test]
    fn validate_full_returns_all_errors() {
        // Two duplicate refs AND a dangling edge
        let graph = SemanticGraph {
            nodes: vec![
                node(0, NodeKind::Module, "a"),
                node(0, NodeKind::Function, "b"), // duplicate NodeRef(0)
            ],
            edges: vec![edge(0, 99, EdgeKind::DependsOn)], // dangling target 99
        };
        let errors = graph.validate_full();
        // Must contain at least DuplicateRef and DanglingEdge
        let has_dup = errors
            .iter()
            .any(|e| matches!(e, GraphValidationError::DuplicateRef(NodeRef(0))));
        let has_dangling = errors.iter().any(|e| {
            matches!(
                e,
                GraphValidationError::DanglingEdge {
                    r#ref: NodeRef(99),
                    role: DanglingRole::Target,
                }
            )
        });
        assert!(
            has_dup,
            "validate_full must include DuplicateRef error; got: {errors:?}"
        );
        assert!(
            has_dangling,
            "validate_full must include DanglingEdge error; got: {errors:?}"
        );
        assert!(
            errors.len() >= 2,
            "validate_full must return all errors, not just one; got: {errors:?}"
        );
    }
}
