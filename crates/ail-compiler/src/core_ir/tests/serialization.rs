use super::helpers::*;

#[test]
fn core_node_with_type_payload_cbor_round_trip() {
    let node = CoreNode {
        source_ref: NodeRef(5),
        kind: CoreNodeKind::Type,
        name: "amount".to_string(),
        ty: Some(CoreType::Int),
        expr: Some(CoreExpr::Var("amount_var".to_string())),
    };
    let bytes = stable_cbor_bytes(&node).expect("encode must succeed");
    let decoded: CoreNode = ciborium::from_reader(bytes.as_slice()).expect("decode must succeed");
    assert_eq!(
        decoded, node,
        "CoreNode with ty+expr must survive CBOR round-trip"
    );
}

// S5: CoreNode without type/expr fields skips those fields in CBOR.
// Verifies backward-compat: a node with None fields produces fewer bytes
// than a node with populated ty/expr (the optional fields are absent).
#[test]
fn core_node_without_type_fields_omits_them_from_cbor() {
    let node_minimal = CoreNode {
        source_ref: NodeRef(0),
        kind: CoreNodeKind::Module,
        name: "m".to_string(),
        ty: None,
        expr: None,
    };
    let node_rich = CoreNode {
        source_ref: NodeRef(0),
        kind: CoreNodeKind::Module,
        name: "m".to_string(),
        ty: Some(CoreType::Bool),
        expr: Some(CoreExpr::Placeholder),
    };
    let bytes_minimal = stable_cbor_bytes(&node_minimal).expect("encode minimal");
    let bytes_rich = stable_cbor_bytes(&node_rich).expect("encode rich");
    // The rich node must produce strictly more bytes (it has extra fields).
    assert!(
        bytes_minimal.len() < bytes_rich.len(),
        "node with ty+expr must encode to more bytes than node without them: {} vs {}",
        bytes_minimal.len(),
        bytes_rich.len()
    );
    // Round-trip the minimal node to confirm no extra keys sneak in.
    let decoded: CoreNode =
        ciborium::from_reader(bytes_minimal.as_slice()).expect("decode minimal");
    assert_eq!(decoded.ty, None, "decoded ty must be None");
    assert_eq!(decoded.expr, None, "decoded expr must be None");
}

// LiteralValue: all 5 variants constructible and Eq is sound.

#[test]
fn new_concurrency_cell_variants_cbor_round_trip() {
    let variants: Vec<CoreExpr> = vec![
        CoreExpr::TaskAwait {
            task: Box::new(CoreExpr::Var("t".to_string())),
        },
        CoreExpr::TaskCancel {
            task: Box::new(CoreExpr::Var("t".to_string())),
        },
        CoreExpr::TaskGroup {
            body: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
        },
        CoreExpr::ChannelNew { capacity: None },
        CoreExpr::ChannelNew { capacity: Some(8) },
        CoreExpr::Select {
            branches: vec![SelectClause {
                channel: Box::new(CoreExpr::Var("ch".to_string())),
                binding: "v".to_string(),
                body: CoreExpr::Var("v".to_string()),
            }],
        },
        CoreExpr::Timeout {
            duration: Box::new(CoreExpr::Var("d".to_string())),
            body: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
        },
        CoreExpr::CellNew {
            init: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
        },
        CoreExpr::CellGet {
            cell: Box::new(CoreExpr::Var("c".to_string())),
        },
        CoreExpr::CellSet {
            cell: Box::new(CoreExpr::Var("c".to_string())),
            value: Box::new(CoreExpr::Literal(LiteralValue::Int(42))),
        },
    ];
    for expr in &variants {
        let bytes = stable_cbor_bytes(expr).expect("encode must succeed");
        let decoded: CoreExpr =
            ciborium::from_reader(bytes.as_slice()).expect("decode must succeed");
        assert_eq!(
            &decoded, expr,
            "CoreExpr::{expr:?} must survive CBOR round-trip"
        );
    }
}

// MatchArm is constructible and Eq.
