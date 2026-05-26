use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::report::VerificationReport;

use super::*;
use crate::core_ir::StageHashes;
use crate::lower::{lower_to_anf, lower_to_core_ir};

fn proven_report() -> VerificationReport {
    VerificationReport {
        entries: vec![],
        ..Default::default()
    }
}

fn anf_for_n(n: usize) -> AnfIr {
    let graph = SemanticGraph {
        nodes: (0..n)
            .map(|i| GraphNode::new(NodeRef(i as u32), NodeKind::Function, format!("fn_{i}")))
            .collect(),
        edges: vec![],
    };
    let core = lower_to_core_ir(&graph, &proven_report()).unwrap();
    lower_to_anf(&core).unwrap()
}

// ── Task 3.1: emit_native rejects unsealed anf_ir_hash ───────────────

// Scenario: anf_ir_hash None → NativeEncodingError.
// Spec: "Unsealed anf_ir_hash is rejected → Err(NativeEncodingError)"
#[test]
fn emit_native_rejects_unsealed_anf_ir_hash() {
    let anf = AnfIr {
        schema_version: crate::anf::ANF_SCHEMA_VERSION,
        bindings: vec![],
        source_map: crate::anf::SourceMap { entries: vec![] },
        stage_hashes: StageHashes {
            graph_snapshot_hash: [0u8; 32],
            verification_report_hash: [0u8; 32],
            core_ir_hash: [1u8; 32],
            anf_ir_hash: None, // unsealed
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        },
    };
    let result = emit_native(&anf);
    assert!(
        matches!(result, Err(CompileError::NativeEncodingError(_))),
        "expected NativeEncodingError for unsealed anf_ir_hash, got {result:?}"
    );
}

// ── Task 3.2: native_hash is sealed after emit_native ─────────────────

// Scenario: native_hash is Some after emit_native.
// Spec: "NativeArtifact.hash_chain.native_hash is Some(...)"
#[test]
fn emit_native_seals_native_hash() {
    let anf = anf_for_n(1);
    let artifact = emit_native(&anf).unwrap();
    assert!(
        artifact.hash_chain.native_hash.is_some(),
        "native_hash must be Some after emit_native"
    );
}

// ── Task 3.3: different AnfIr inputs produce different native_hash ─────

// Triangulate: different inputs → different hashes.
#[test]
fn different_anf_produces_different_native_hash() {
    let a1 = emit_native(&anf_for_n(1)).unwrap();
    let a2 = emit_native(&anf_for_n(2)).unwrap();
    assert_ne!(
        a1.hash_chain.native_hash, a2.hash_chain.native_hash,
        "different AnfIr inputs must produce different native_hashes"
    );
}

// ── Task 3.4: provenance len == binding count; empty → empty ──────────

// Scenario: N bindings → N provenance entries.
// Spec: "NativeArtifact.provenance.len() equals N"
#[test]
fn provenance_len_equals_binding_count() {
    for n in [0usize, 1, 3, 5] {
        let anf = anf_for_n(n);
        let artifact = emit_native(&anf).unwrap();
        assert_eq!(
            artifact.provenance.len(),
            n,
            "provenance must have {n} entries for {n}-binding AnfIr"
        );
    }
}

// Scenario: empty ANF → empty provenance.
// Spec: "Empty AnfIr produces empty provenance"
#[test]
fn empty_anf_produces_empty_provenance() {
    let anf = anf_for_n(0);
    let artifact = emit_native(&anf).unwrap();
    assert!(
        artifact.provenance.is_empty(),
        "empty AnfIr must produce empty provenance map"
    );
}

// ── TASK-A0: Extended arithmetic ops — RED ────────────────────────────
// These all currently hit the catch-all `_ =>` arm and emit trap,
// producing the same bytes as Placeholder.  They must fail until A1 lands.

fn anf_with_call2(func: &str, lhs: i64, rhs: i64) -> AnfIr {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "x".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(lhs))),
            body: Box::new(AnfExpr::Let {
                name: "y".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(rhs))),
                body: Box::new(AnfExpr::Call {
                    func: func.to_string(),
                    args: vec!["x".to_string(), "y".to_string()],
                }),
            }),
        },
    })
}

fn anf_with_call1(func: &str, operand: i64) -> AnfIr {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "x".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(operand))),
            body: Box::new(AnfExpr::Call {
                func: func.to_string(),
                args: vec!["x".to_string()],
            }),
        },
    })
}

fn placeholder_anf() -> AnfIr {
    use crate::anf::{AnfBinding, AnfExpr};
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Placeholder,
    })
}

#[test]
fn native_div_differs_from_placeholder() {
    let art = emit_native(&anf_with_call2("i64.div_s", 10, 2)).unwrap();
    let ph = emit_native(&placeholder_anf()).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "i64.div_s must produce different bytes than Placeholder"
    );
}

#[test]
fn native_rem_differs_from_placeholder() {
    let art = emit_native(&anf_with_call2("i64.rem_s", 10, 3)).unwrap();
    let ph = emit_native(&placeholder_anf()).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "i64.rem_s must produce different bytes than Placeholder"
    );
}

#[test]
fn native_eq_differs_from_placeholder() {
    let art = emit_native(&anf_with_call2("i64.eq", 5, 5)).unwrap();
    let ph = emit_native(&placeholder_anf()).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "i64.eq must produce different bytes than Placeholder"
    );
}

#[test]
fn native_neg_differs_from_placeholder() {
    let art = emit_native(&anf_with_call1("i64.neg", 7)).unwrap();
    let ph = emit_native(&placeholder_anf()).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "i64.neg must produce different bytes than Placeholder"
    );
}

#[test]
fn native_eqz_differs_from_placeholder() {
    let art = emit_native(&anf_with_call1("i64.eqz", 0)).unwrap();
    let ph = emit_native(&placeholder_anf()).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "i64.eqz must produce different bytes than Placeholder"
    );
}

// ── TASK-B0: If + ShortCircuit tests — RED ────────────────────────────
// These hit the catch-all `_ =>` trap arm until B1 lands.

fn anf_with_if(cond_val: bool, then_val: i64, else_val: i64) -> AnfIr {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "c".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Bool(cond_val))),
            body: Box::new(AnfExpr::If {
                cond: "c".to_string(),
                then_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(then_val))),
                else_branch: Box::new(AnfExpr::Literal(LiteralValue::Int(else_val))),
            }),
        },
    })
}

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

fn anf_for_binding(binding: crate::anf::AnfBinding) -> AnfIr {
    use crate::anf::SourceMap;
    AnfIr {
        schema_version: crate::anf::ANF_SCHEMA_VERSION,
        source_map: SourceMap::from_bindings(std::slice::from_ref(&binding)),
        bindings: vec![binding],
        stage_hashes: StageHashes {
            graph_snapshot_hash: [0u8; 32],
            verification_report_hash: [0u8; 32],
            core_ir_hash: [1u8; 32],
            anf_ir_hash: Some([2u8; 32]),
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        },
    }
}

// Helper: emit native for a single Int literal binding with a FIXED name
// so that two calls with different values produce identical symbol tables
// and any byte difference is purely from code content.
fn anf_with_int_literal(n: i64) -> AnfIr {
    use crate::anf::AnfBinding;
    use crate::core_ir::LiteralValue;
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_lit".to_string(), // fixed name — code difference is the only variable
        expr: crate::anf::AnfExpr::Literal(LiteralValue::Int(n)),
    })
}

// C-5a: Two different Int literals must produce different native_bytes.
// RED: currently both are trap stubs → byte-identical object files.
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

fn anf_with_record(fields: Vec<(&str, i64)>) -> AnfIr {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let field_exprs: Vec<(String, AnfExpr)> = fields
        .into_iter()
        .map(|(f, v)| (f.to_string(), AnfExpr::Literal(LiteralValue::Int(v))))
        .collect();
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::RecordNew {
            fields: field_exprs,
        },
    })
}

#[test]
fn native_record_new_differs_from_placeholder() {
    let art = emit_native(&anf_with_record(vec![("x", 1), ("y", 2)])).unwrap();
    let ph = emit_native(&placeholder_anf()).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "RecordNew must produce different bytes than Placeholder"
    );
    assert_eq!(
        infer_cranelift_return_type(&crate::anf::AnfExpr::RecordNew {
            fields: vec![(
                "x".to_string(),
                crate::anf::AnfExpr::Literal(crate::core_ir::LiteralValue::Int(1))
            )],
        }),
        Some(cranelift_codegen::ir::types::I64)
    );
}

#[test]
fn native_field_get_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "r".to_string(),
            value: Box::new(AnfExpr::RecordNew {
                fields: vec![("x".to_string(), AnfExpr::Literal(LiteralValue::Int(10)))],
            }),
            body: Box::new(AnfExpr::FieldGet {
                record: "r".to_string(),
                field: "x".to_string(),
            }),
        },
    });
    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "FieldGet must produce different bytes than Placeholder"
    );
}

#[test]
fn native_field_update_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "r".to_string(),
            value: Box::new(AnfExpr::RecordNew {
                fields: vec![("x".to_string(), AnfExpr::Literal(LiteralValue::Int(1)))],
            }),
            body: Box::new(AnfExpr::FieldUpdate {
                record: "r".to_string(),
                field: "x".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(99))),
            }),
        },
    });
    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "FieldUpdate must produce different bytes than Placeholder"
    );
}

#[test]
fn native_record_zero_fields_compiles() {
    let art = emit_native(&anf_with_record(vec![]));
    assert!(art.is_ok(), "RecordNew{{[]}} must compile without panic");
}

// ── TASK-H0: VariantNew / ListNew / TupleNew ──────────────────────────

#[test]
fn native_variant_new_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::VariantNew {
            tag: "Ok".to_string(),
            payload: Some(Box::new(AnfExpr::Literal(LiteralValue::Int(42)))),
        },
    });
    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "VariantNew must produce different bytes than Placeholder"
    );
}

#[test]
fn native_list_new_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::ListNew(vec![
            AnfExpr::Literal(LiteralValue::Int(1)),
            AnfExpr::Literal(LiteralValue::Int(2)),
        ]),
    });
    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "ListNew must produce different bytes than Placeholder"
    );
}

#[test]
fn native_tuple_new_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::TupleNew(vec![
            AnfExpr::Literal(LiteralValue::Int(3)),
            AnfExpr::Literal(LiteralValue::Int(4)),
        ]),
    });
    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "TupleNew must produce different bytes than Placeholder"
    );
}

#[test]
fn native_variant_two_tags_differ() {
    use crate::anf::{AnfBinding, AnfExpr};
    let make_variant = |tag: &str| {
        anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::VariantNew {
                tag: tag.to_string(),
                payload: None,
            },
        })
    };
    let art_ok = emit_native(&make_variant("Ok")).unwrap();
    let art_err = emit_native(&make_variant("Err")).unwrap();
    assert_ne!(
        art_ok.native_bytes, art_err.native_bytes,
        "VariantNew('Ok') and VariantNew('Err') must produce different bytes (different tag ids)"
    );
}

// ── TASK-I0: EffectCall — RED ─────────────────────────────────────────

#[test]
fn native_effect_call_differs_from_placeholder() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "id".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::EffectCall {
                capability: "db".to_string(),
                func: "read".to_string(),
                args: vec!["id".to_string()],
            }),
        },
    });
    let ph = emit_native(&placeholder_anf()).unwrap();
    let art = emit_native(&anf).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "EffectCall must produce different bytes than Placeholder"
    );
}

#[test]
fn native_effect_call_two_capabilities_differ() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let make_effect = |cap: &str| {
        anf_for_binding(AnfBinding {
            source_ref: NodeRef(0),
            name: "fn_op".to_string(),
            expr: AnfExpr::Let {
                name: "id".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                body: Box::new(AnfExpr::EffectCall {
                    capability: cap.to_string(),
                    func: "read".to_string(),
                    args: vec!["id".to_string()],
                }),
            },
        })
    };
    let art_db = emit_native(&make_effect("db")).unwrap();
    let art_fs = emit_native(&make_effect("fs")).unwrap();
    assert_ne!(
        art_db.native_bytes, art_fs.native_bytes,
        "EffectCall('db') and EffectCall('fs') must produce different bytes"
    );
}

#[test]
fn native_effect_call_native_hash_is_some() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let anf = anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "id".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::EffectCall {
                capability: "db".to_string(),
                func: "read".to_string(),
                args: vec!["id".to_string()],
            }),
        },
    });
    let art = emit_native(&anf).unwrap();
    assert!(
        art.hash_chain.native_hash.is_some(),
        "native_hash must be Some for EffectCall"
    );
}

// ── TASK-J0: Lambda closure env construction ──────────────────────────
//
// PR2 invariant: captures must NOT be silently dropped.
//
// Scenario map:
//   J-1: Lambda with no captures → bare fn-ptr, compiles, differs from Placeholder.
//   J-2: Two no-capture lambdas with different bodies → different bytes.
//   J-3: Lambda with one capture → compiles without error.
//   J-4: Lambda with captures → different bytes than the same lambda with no captures
//        (closure env allocation changes the emitted code).
//   J-5: Lambda with two captures → different bytes than lambda with one capture
//        (env size and stored values differ).

fn anf_lambda_no_captures(body_val: i64) -> AnfIr {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["p".to_string()],
            captures: vec![],
            body: Box::new(AnfExpr::Literal(LiteralValue::Int(body_val))),
        },
    })
}

fn anf_lambda_one_capture(cap_val: i64) -> AnfIr {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    // let x = cap_val in (lambda captures=[x] params=[p] body=Var("p"))
    // body=Var("p") keeps the inner function compilable; the closure env
    // carries x's value by value via the outer ctx.
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Let {
            name: "x".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(cap_val))),
            body: Box::new(AnfExpr::Lambda {
                params: vec!["p".to_string()],
                captures: vec!["x".to_string()],
                body: Box::new(AnfExpr::Var("p".to_string())),
            }),
        },
    })
}

fn anf_lambda_returning_param_body(body: crate::anf::AnfExpr) -> AnfIr {
    use crate::anf::{AnfBinding, AnfExpr};
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["p".to_string()],
            captures: vec![],
            body: Box::new(body),
        },
    })
}

// J-1: Lambda with no captures compiles and differs from Placeholder.
#[test]
fn native_lambda_no_captures_differs_from_placeholder() {
    let art = emit_native(&anf_lambda_no_captures(7)).unwrap();
    let ph = emit_native(&placeholder_anf()).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "Lambda with no captures must produce different bytes than Placeholder"
    );
}

// J-2: Two no-capture lambdas with different body constants differ in bytes.
#[test]
fn native_lambda_no_captures_body_triangulate() {
    let art1 = emit_native(&anf_lambda_no_captures(1)).unwrap();
    let art2 = emit_native(&anf_lambda_no_captures(99)).unwrap();
    assert_ne!(
        art1.native_bytes, art2.native_bytes,
        "Lambda no-capture: body constant 1 vs 99 must produce different bytes"
    );
}

// J-3: Lambda with one capture compiles without error.
#[test]
fn native_lambda_with_one_capture_compiles() {
    let result = emit_native(&anf_lambda_one_capture(42));
    assert!(
        result.is_ok(),
        "Lambda with one capture must compile without error: {:?}",
        result.err()
    );
}

// J-4: Lambda with a capture produces different bytes than the same lambda
// without captures.  The closure env allocation and stores change the IR.
#[test]
fn native_lambda_with_capture_differs_from_no_capture() {
    let with_cap = emit_native(&anf_lambda_one_capture(42)).unwrap();
    // Build a structurally similar no-capture lambda for comparison.
    let without_cap = emit_native(&anf_lambda_no_captures(42)).unwrap();
    assert_ne!(
        with_cap.native_bytes, without_cap.native_bytes,
        "Lambda with captures must produce different bytes than lambda with no captures: \
         closure env allocation must be emitted, not silently dropped"
    );
}

#[test]
fn native_lambda_return_var_param_compiles() {
    use crate::anf::AnfExpr;
    let anf =
        anf_lambda_returning_param_body(AnfExpr::Return(Box::new(AnfExpr::Var("p".to_string()))));
    let result = emit_native(&anf);
    assert!(
        result.is_ok(),
        "Lambda body Return(Var(param)) must infer an I64 return: {:?}",
        result.err()
    );
}

#[test]
fn native_lambda_let_wrapped_return_var_param_compiles() {
    use crate::anf::AnfExpr;
    use crate::core_ir::LiteralValue;
    let anf = anf_lambda_returning_param_body(AnfExpr::Let {
        name: "tmp".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        body: Box::new(AnfExpr::Return(Box::new(AnfExpr::Var("p".to_string())))),
    });
    let result = emit_native(&anf);
    assert!(
        result.is_ok(),
        "Lambda body Let(... Return(Var(param))) must infer an I64 return: {:?}",
        result.err()
    );
}

#[test]
fn native_lambda_seq_wrapped_return_var_param_compiles() {
    use crate::anf::AnfExpr;
    use crate::core_ir::LiteralValue;
    let anf = anf_lambda_returning_param_body(AnfExpr::Seq(vec![
        AnfExpr::Literal(LiteralValue::Int(1)),
        AnfExpr::Return(Box::new(AnfExpr::Var("p".to_string()))),
    ]));
    let result = emit_native(&anf);
    assert!(
        result.is_ok(),
        "Lambda body Seq(... Return(Var(param))) must infer an I64 return: {:?}",
        result.err()
    );
}

// J-5a: NativeDataLayout must set needs_heap_alloc for Lambda with captures.
// Proves the pre-scan correctly identifies that a closure env requires heap
// allocation, which in turn drives __ail_malloc import in emit_native.
#[test]
fn native_data_layout_lambda_with_captures_needs_heap_alloc() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["p".to_string()],
            captures: vec!["x".to_string()],
            body: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        },
    };
    let layout = NativeDataLayout::for_bindings(&[binding]);
    assert!(
        layout.needs_heap_alloc,
        "Lambda with non-empty captures must set needs_heap_alloc in NativeDataLayout"
    );
}

// J-5b: NativeDataLayout must NOT set needs_heap_alloc for Lambda with no captures.
// Negative test: empty captures → no env allocation needed.
#[test]
fn native_data_layout_lambda_no_captures_no_heap_alloc() {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["p".to_string()],
            captures: vec![],
            body: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        },
    };
    let layout = NativeDataLayout::for_bindings(&[binding]);
    assert!(
        !layout.needs_heap_alloc,
        "Lambda with empty captures must not set needs_heap_alloc in NativeDataLayout"
    );
}

// ── Wave 11A: Native Bytes literal emit ───────────────────────────────
//
// Scenario map:
//   B-1: Bytes literal compiles and produces different bytes than Placeholder.
//   B-2: Two different byte slices produce different native_bytes.
//   B-3: Same byte slice interns to the same index (deduplication).
//   B-4: infer_cranelift_return_type returns I64 for Bytes.
//   B-5: Empty byte slice compiles without panic.

fn anf_with_bytes(data: Vec<u8>) -> AnfIr {
    use crate::anf::{AnfBinding, AnfExpr};
    use crate::core_ir::LiteralValue;
    anf_for_binding(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_op".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Bytes(data)),
    })
}

// B-1: Bytes literal compiles and differs from Placeholder.
#[test]
fn native_bytes_literal_differs_from_placeholder() {
    let art = emit_native(&anf_with_bytes(vec![0xDE, 0xAD, 0xBE, 0xEF])).unwrap();
    let ph = emit_native(&placeholder_anf()).unwrap();
    assert_ne!(
        art.native_bytes, ph.native_bytes,
        "Literal(Bytes([0xDE, 0xAD, 0xBE, 0xEF])) must produce different bytes than Placeholder"
    );
}

// B-2: Two different byte slices produce different native_bytes (triangulation).
#[test]
fn native_bytes_two_slices_differ() {
    let art1 = emit_native(&anf_with_bytes(vec![1, 2, 3])).unwrap();
    let art2 = emit_native(&anf_with_bytes(vec![4, 5, 6])).unwrap();
    assert_ne!(
        art1.native_bytes, art2.native_bytes,
        "Bytes([1,2,3]) and Bytes([4,5,6]) must produce different native_bytes"
    );
}

// B-3: Same byte slice is interned once (deduplication in NativeDataLayout).
#[test]
fn native_bytes_same_slice_deduplicated() {
    let mut layout = NativeDataLayout::default();
    let idx1 = layout.intern_bytes(&[0xCA, 0xFE]);
    let idx2 = layout.intern_bytes(&[0xCA, 0xFE]);
    assert_eq!(idx1, idx2, "Same byte slice must intern to same index");
    assert_eq!(
        layout.bytes_table.len(),
        1,
        "Only one bytes_table entry should exist for duplicate slices"
    );
}

// B-4: infer_cranelift_return_type returns I64 for Bytes.
#[test]
fn native_bytes_infer_return_type_is_i64() {
    use crate::anf::AnfExpr;
    use crate::core_ir::LiteralValue;
    use cranelift_codegen::ir::types;
    let expr = AnfExpr::Literal(LiteralValue::Bytes(vec![1, 2, 3]));
    assert_eq!(
        infer_cranelift_return_type(&expr),
        Some(types::I64),
        "infer_cranelift_return_type for Literal(Bytes) must return Some(I64)"
    );
}

// B-5: Empty byte slice compiles without panic.
#[test]
fn native_bytes_empty_slice_compiles() {
    let result = emit_native(&anf_with_bytes(vec![]));
    assert!(
        result.is_ok(),
        "Literal(Bytes([])) must compile without panic: {:?}",
        result.err()
    );
}
