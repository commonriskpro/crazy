use super::helpers::*;

#[test]
fn native_if_true_returns_then_branch() {
    let art = emit_native(&anf_with_if(true, 1, 2)).unwrap();
    let ph = emit_native(&placeholder_anf()).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "If with Bool(true) cond must produce different bytes than Placeholder"
    );
}

#[test]
fn native_if_false_returns_else_branch() {
    let art_true = emit_native(&anf_with_if(true, 1, 2)).unwrap();
    let art_false = emit_native(&anf_with_if(false, 1, 2)).unwrap();
    assert_ne!(
        art_true.native_bytes, art_false.native_bytes,
        "If with Bool(true) and Bool(false) cond must produce different bytes"
    );
}

#[test]
fn native_if_no_result_compiles() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "c".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
            body: Box::new(AnfExpr::If {
                cond: "c".to_string(),
                then_branch: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
                else_branch: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
            }),
        },
    });
    assert!(
        emit_native(&anf).is_ok(),
        "If with Unit branches must compile without panic"
    );
}

#[test]
fn native_if_infer_return_type_is_i64() {
    use crate::anf::AnfExpr;
    use crate::core_ir::LiteralValue;
    use cranelift_codegen::ir::types;
    let expr = AnfExpr::If {
        cond: "c".to_string(),
        then_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        else_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(2))),
    };
    assert_eq!(
        infer_cranelift_return_type(&expr),
        Some(types::I64),
        "infer_cranelift_return_type for If{{Int, Int}} must return Some(I64)"
    );
}

#[test]
fn native_short_circuit_and_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "t".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
            body: Box::new(AnfExpr::ShortCircuitAnd {
                left: "t".to_string(),
                right: Box::new(AnfExpr::Literal(LiteralValue::Int(7))),
            }),
        },
    });
    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "ShortCircuitAnd must produce different bytes than Placeholder"
    );
}

#[test]
fn native_short_circuit_or_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "f".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
            body: Box::new(AnfExpr::ShortCircuitOr {
                left: "f".to_string(),
                right: Box::new(AnfExpr::Literal(LiteralValue::Int(3))),
            }),
        },
    });
    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "ShortCircuitOr must produce different bytes than Placeholder"
    );
}

// ── TASK-B1: Native expression lowering tests (TDD RED) ───────────────
// Spec scenarios C-5a, C-5b, C-5c, and C-5d.

#[test]
fn two_int_literal_bindings_produce_different_native_bytes() {
    let art1 = emit_native(&anf_with_int_literal(1)).unwrap();
    let art2 = emit_native(&anf_with_int_literal(2)).unwrap();
    assert_ne!(
        art1.native_bytes, art2.native_bytes,
        "Literal(Int(1)) and Literal(Int(2)) must produce different native code bytes"
    );
}

// C-5b: Int literal binding must produce different bytes than a Placeholder.
// RED: currently both are trap stubs → same bytes (same name, same trap code).
#[test]
fn emit_native_int_literal_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr};
    let lit_art = emit_native(&anf_with_int_literal(42)).unwrap();
    let placeholder_anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_lit".to_string(), // same name → only code differs
        expr: AnfExpr::Placeholder,
    });
    let placeholder_art = emit_native(&placeholder_anf).unwrap();
    assert_ne!(
        lit_art.native_bytes, placeholder_art.native_bytes,
        "Literal(Int(42)) must produce different native code than Placeholder (trap stub)"
    );
}

// C-5c: Let{x=Int(3), y=Int(4), body=Call{"i64.add",[x,y]}} must produce
// different bytes than a plain Placeholder stub with the same function name.
// RED: currently Let+Add → trap stub → same bytes as Placeholder.
#[test]
fn native_lowering_let_int_add_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let add_binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_add".to_string(),
        expr: AnfExpr::Let {
            name: "x".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(3))),
            body: Box::new(AnfExpr::Let {
                name: "y".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(4))),
                body: Box::new(AnfExpr::Call {
                    func: "i64.add".to_string(),
                    args: vec!["x".to_string(), "y".to_string()],
                }),
            }),
        },
    };
    let placeholder_binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_add".to_string(), // same name to isolate code difference
        expr: AnfExpr::Placeholder,
    };
    let add_art = emit_native(&anf_for_binding(add_binding)).unwrap();
    let placeholder_art = emit_native(&anf_for_binding(placeholder_binding)).unwrap();
    assert!(
        !add_art.native_bytes.is_empty(),
        "native_bytes must be non-empty"
    );
    assert_ne!(
        add_art.native_bytes, placeholder_art.native_bytes,
        "Let+Add must produce different code than a Placeholder trap stub"
    );
}

// ── TASK-D0: Loop / Break / Continue / WhileLoop — RED ───────────────

#[test]
fn native_loop_break_int_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Loop {
            body: Box::new(AnfExpr::Break {
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(42))),
            }),
        },
    });
    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "Loop{{Break{{Int(42)}}}} must produce different bytes than Placeholder"
    );
    assert_eq!(
        infer_cranelift_return_type(&AnfExpr::Loop {
            body: Box::new(AnfExpr::Break {
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(42))),
            }),
        }),
        Some(cranelift_codegen::ir::types::I64)
    );
}

#[test]
fn native_loop_break_unit_compiles() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Loop {
            body: Box::new(AnfExpr::Break {
                value: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
            }),
        },
    });
    assert!(
        emit_native(&anf).is_ok(),
        "Loop{{Break{{Unit}}}} must compile without panic"
    );
}

#[test]
fn native_while_loop_compiles() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "c".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Bool(false))),
            body: Box::new(AnfExpr::WhileLoop {
                cond: "c".to_string(),
                body: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            }),
        },
    });
    assert!(
        emit_native(&anf).is_ok(),
        "WhileLoop with Bool(false) cond must compile"
    );
}

#[test]
fn native_continue_compiles() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    // Loop { Seq([Continue, Break{Int(1)}]) }
    // Continue is unreachable after first iteration but CFG must be valid.
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Loop {
            body: Box::new(AnfExpr::Seq(vec![
                AnfExpr::Continue,
                AnfExpr::Break {
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                },
            ])),
        },
    });
    assert!(
        emit_native(&anf).is_ok(),
        "Loop{{Continue; Break}} must compile without panic"
    );
}

// ── TASK-F0: Literal(Text) + NativeDataLayout — RED ──────────────────

#[test]
fn native_text_literal_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Text("hello".to_string())),
    });
    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "Literal(Text(\"hello\")) must produce different bytes than Placeholder"
    );
}

#[test]
fn native_text_literal_two_strings_differ() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let make = |s: &str| {
        anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Literal(LiteralValue::Text(s.to_string())),
        })
    };
    let art_hello = emit_native(&make("hello")).unwrap();
    let art_world = emit_native(&make("world")).unwrap();
    assert_ne!(
        art_hello.native_bytes, art_world.native_bytes,
        "Literal(Text(\"hello\")) and Literal(Text(\"world\")) must produce different bytes"
    );
}

#[test]
fn native_text_literal_same_string_deduplicated() {
    // Two bindings both using the same string literal should intern it once.
    // Test: NativeDataLayout interns the same string to same index.
    // RED: NativeDataLayout doesn't exist yet.
    let mut layout = NativeDataLayout::default();
    let idx1 = layout.intern("hello");
    let idx2 = layout.intern("hello");
    assert_eq!(idx1, idx2, "Same string must intern to same index");
    assert_eq!(layout.ordered.len(), 1, "Only one data object should exist");
}

// ── TASK-E0: Match — RED ──────────────────────────────────────────────

#[test]
fn native_match_int_arm_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr, AnfMatchArm};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "x".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::Match {
                scrutinee: "x".to_string(),
                arms: vec![
                    AnfMatchArm {
                        pattern: "1".to_string(),
                        body: AnfExpr::Literal(LiteralValue::Int(10)),
                    },
                    AnfMatchArm {
                        pattern: "_".to_string(),
                        body: AnfExpr::Literal(LiteralValue::Int(99)),
                    },
                ],
            }),
        },
    });
    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "Match with i64 arm must produce different bytes than Placeholder"
    );
}

#[test]
fn native_match_wildcard_only_compiles() {
    use crate::anf::{AnfBinding, AnfExpr, AnfMatchArm};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "x".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Match {
                scrutinee: "x".to_string(),
                arms: vec![AnfMatchArm {
                    pattern: "_".to_string(),
                    body: AnfExpr::Literal(LiteralValue::Int(0)),
                }],
            }),
        },
    });
    assert!(
        emit_native(&anf).is_ok(),
        "Match with wildcard only must compile"
    );
}

#[test]
fn native_match_bool_arm() {
    use crate::anf::{AnfBinding, AnfExpr, AnfMatchArm};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "b".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
            body: Box::new(AnfExpr::Match {
                scrutinee: "b".to_string(),
                arms: vec![
                    AnfMatchArm {
                        pattern: "true".to_string(),
                        body: AnfExpr::Literal(LiteralValue::Int(1)),
                    },
                    AnfMatchArm {
                        pattern: "false".to_string(),
                        body: AnfExpr::Literal(LiteralValue::Int(0)),
                    },
                ],
            }),
        },
    });
    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "Match with bool arms must produce different bytes than Placeholder"
    );
    assert_eq!(
        infer_cranelift_return_type(&AnfExpr::Match {
            scrutinee: "b".to_string(),
            arms: vec![crate::anf::AnfMatchArm {
                pattern: "true".to_string(),
                body: AnfExpr::Literal(LiteralValue::Int(1))
            },],
        }),
        Some(cranelift_codegen::ir::types::I64)
    );
}

#[test]
fn native_match_empty_arms_compiles() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "x".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Match {
                scrutinee: "x".to_string(),
                arms: vec![],
            }),
        },
    });
    assert!(
        emit_native(&anf).is_ok(),
        "Match with empty arms must compile (produces trap)"
    );
}

// ── TASK-C0: Seq, RuntimeCheck — RED ──────────────────────────────────

#[test]
fn native_seq_emits_last_value() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    // Triangulation: two Seqs that differ only in last element must produce
    // different bytes once Seq is properly lowered.
    // RED: currently both hit catch-all trap → identical bytes.
    let seq_a = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Seq(vec![
            AnfExpr::Literal(LiteralValue::Int(1)),
            AnfExpr::Literal(LiteralValue::Int(2)),
        ]),
    });
    let seq_b = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Seq(vec![
            AnfExpr::Literal(LiteralValue::Int(1)),
            AnfExpr::Literal(LiteralValue::Int(5)),
        ]),
    });
    let art_a = emit_native(&seq_a).unwrap();
    let art_b = emit_native(&seq_b).unwrap();
    assert_ne!(
        art_a.native_bytes, art_b.native_bytes,
        "Seq([Int(1), Int(2)]) and Seq([Int(1), Int(5)]) must produce different bytes"
    );
    // infer_return_type should be Some for the last element
    assert_eq!(
        infer_cranelift_return_type(&AnfExpr::Seq(vec![
            AnfExpr::Literal(LiteralValue::Int(1)),
            AnfExpr::Literal(LiteralValue::Int(2)),
        ])),
        Some(cranelift_codegen::ir::types::I64)
    );
}

#[test]
fn native_seq_empty_compiles() {
    use crate::anf::{AnfBinding, AnfExpr};
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Seq(vec![]),
    });
    assert!(
        emit_native(&anf).is_ok(),
        "Seq([]) must compile without panic"
    );
}

#[test]
fn native_runtime_check_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "ok".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::RuntimeCheck {
                check_ref: "c1".to_string(),
                cond: "ok".to_string(),
                msg: "err".to_string(),
            }),
        },
    });
    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "RuntimeCheck must produce different bytes than Placeholder"
    );
}

// ── TASK-G0: RecordNew / FieldGet / FieldUpdate — RED ────────────────
