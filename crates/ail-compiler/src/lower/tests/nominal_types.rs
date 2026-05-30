use super::*;

// ── nominal_to_core_type ─────────────────────────────────────────────

// S6 (partial): all 20 known nominals map to their CoreType variant.
#[test]
fn all_known_nominals_map_to_correct_core_type() {
    use crate::core_ir::{CoreType, ResourceMode};
    let cases: &[(&str, CoreType)] = &[
        ("Unit", CoreType::Unit),
        ("Never", CoreType::Never),
        ("Bool", CoreType::Bool),
        ("Int", CoreType::Int),
        ("UInt", CoreType::UInt),
        ("Float", CoreType::Float),
        ("Text", CoreType::Text),
        ("Bytes", CoreType::Bytes),
        ("Record", CoreType::Record),
        ("Variant", CoreType::Variant),
        ("Tuple", CoreType::Tuple),
        ("List", CoreType::List(Box::new(CoreType::Generic(None)))),
        (
            "Map",
            CoreType::Map(
                Box::new(CoreType::Generic(None)),
                Box::new(CoreType::Generic(None)),
            ),
        ),
        ("Set", CoreType::Set(Box::new(CoreType::Generic(None)))),
        (
            "Option",
            CoreType::Option(Box::new(CoreType::Generic(None))),
        ),
        (
            "Result",
            CoreType::Result(
                Box::new(CoreType::Generic(None)),
                Box::new(CoreType::Generic(None)),
            ),
        ),
        (
            "Function",
            CoreType::Function {
                params: vec![],
                ret: Box::new(CoreType::Generic(None)),
                effects: vec![],
            },
        ),
        (
            "Handle",
            CoreType::Handle {
                resource: Box::new(CoreType::Generic(None)),
                mode: ResourceMode::Copy,
            },
        ),
        (
            "Refinement",
            CoreType::Refinement {
                base: Box::new(CoreType::Generic(None)),
                predicate: String::new(),
            },
        ),
        ("Generic", CoreType::Generic(None)),
    ];
    for (nominal, expected) in cases {
        assert_eq!(
            nominal_to_core_type(nominal),
            *expected,
            "nominal {nominal:?} must map to {expected:?}"
        );
    }
}

// S7: unknown nominal falls back to CoreType::Generic(None).
#[test]
fn unknown_nominal_maps_to_generic() {
    use crate::core_ir::CoreType;
    assert_eq!(nominal_to_core_type("Exotic"), CoreType::Generic(None));
    assert_eq!(nominal_to_core_type(""), CoreType::Generic(None));
    assert_eq!(nominal_to_core_type("int"), CoreType::Generic(None)); // case-sensitive
}

// ── G2 lower_to_core_ir with type_facts ──────────────────────────────

// S6: lower_to_core_ir populates CoreType::Int for a node with
// type_facts.nominal = "Int".
#[test]
fn lower_to_core_ir_populates_core_type_from_type_facts() {
    use crate::core_ir::CoreType;
    use ail_core::semantic_graph::TypeFacts;

    let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "amount");
    node.type_facts = Some(TypeFacts {
        nominal: "Int".to_string(),
        generics: vec![],
    });
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let report = proven_report();
    let core = lower_to_core_ir(&graph, &report).unwrap();
    assert_eq!(
        core.nodes[0].ty,
        Some(CoreType::Int),
        "node with TypeFacts.nominal=Int must get ty=Some(CoreType::Int)"
    );
}

#[test]
fn lower_to_core_ir_maps_literal_function_value_to_expr() {
    use crate::core_ir::{CoreExpr, LiteralValue};
    use ail_core::semantic_graph::RuntimeCheckMeta;

    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.answer");
    node.return_type = Some("Int".to_string());
    node.runtime_checks = Some(vec![RuntimeCheckMeta {
        predicate: "literal:i64=42".to_string(),
        hash: "literal-hash".to_string(),
    }]);
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };

    let core = lower_to_core_ir(&graph, &proven_report()).unwrap();

    assert_eq!(
        core.nodes[0].expr,
        Some(CoreExpr::Literal(LiteralValue::Int(42)))
    );
}

#[test]
fn lower_to_core_ir_parses_function_body_expr() {
    use crate::core_ir::CoreExpr;

    let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.add");
    node.body_expr = Some("add(x, y)".to_string());
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };

    let core = lower_to_core_ir(&graph, &proven_report()).unwrap();

    assert_eq!(
        core.nodes[0].expr,
        Some(CoreExpr::Add(
            Box::new(CoreExpr::Var("x".to_string())),
            Box::new(CoreExpr::Var("y".to_string()))
        ))
    );
}

// S8: lower_to_core_ir leaves ty = None for nodes without type_facts.
#[test]
fn lower_to_core_ir_leaves_ty_none_without_type_facts() {
    let graph = one_node_graph(); // GraphNode::new — type_facts is None
    let report = proven_report();
    let core = lower_to_core_ir(&graph, &report).unwrap();
    assert_eq!(
        core.nodes[0].ty, None,
        "node without TypeFacts must have ty=None"
    );
}

// S7 (lowering): lower_to_core_ir uses Generic for unknown nominals.
#[test]
fn lower_to_core_ir_uses_generic_for_unknown_nominal() {
    use crate::core_ir::CoreType;
    use ail_core::semantic_graph::TypeFacts;

    let mut node = GraphNode::new(NodeRef(0), NodeKind::Type, "exotic");
    node.type_facts = Some(TypeFacts {
        nominal: "Exotic".to_string(),
        generics: vec![],
    });
    let graph = SemanticGraph {
        nodes: vec![node],
        edges: vec![],
    };
    let report = proven_report();
    let core = lower_to_core_ir(&graph, &report).unwrap();
    assert_eq!(
        core.nodes[0].ty,
        Some(CoreType::Generic(None)),
        "unknown nominal must produce CoreType::Generic(None)"
    );
}

// expr is always None after lower_to_core_ir (deferred phase).
#[test]
fn lower_to_core_ir_expr_is_always_none() {
    let graph = one_node_graph();
    let report = proven_report();
    let core = lower_to_core_ir(&graph, &report).unwrap();
    assert!(
        core.nodes[0].expr.is_none(),
        "expr must be None after lower_to_core_ir (deferred to expression lowering)"
    );
}

// ── G20: Expression body lowering tests ──────────────────────────────

// Helper: lower a single CoreExpr to AnfExpr (no prior bindings).
