use super::*;

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
    let decoded: CapabilityArgBinding = codec.decode(&bytes).expect("decode CapabilityArgBinding");
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
