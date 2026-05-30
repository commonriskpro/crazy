use super::*;

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
