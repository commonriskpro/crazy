// Tests for the WASM emission stage.
// Declared from wasm.rs as: #[cfg(test)] #[path = "wasm_tests.rs"] mod tests;

use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::report::VerificationReport;

use super::emit_wasm;
use crate::anf::{ANF_SCHEMA_VERSION, AnfBinding, AnfExpr, AnfIr, SourceMap};
use crate::core_ir::{LiteralValue, StageHashes};
use crate::error::CompileError;
use crate::lower::{lower_to_anf, lower_to_core_ir};
use crate::wasm_abi::{
    EffectDataLayout, WasmScalarType, WasmSignature, WasmTypeDescriptor, binding_params,
    collect_free_vars, derive_wasm_type,
};
use crate::wasm_sections::build_type_section;

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

// Task 3.3 inline unit tests ──────────────────────────────────────────

// Scenario: anf_ir_hash None → EncodingError.
// Proves the pre-condition gate fires correctly.
#[test]
fn emit_wasm_rejects_unsealed_anf_ir_hash() {
    let anf = AnfIr {
        schema_version: ANF_SCHEMA_VERSION,
        bindings: vec![],
        source_map: SourceMap { entries: vec![] },
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
    let result = emit_wasm(&anf);
    assert!(
        matches!(result, Err(CompileError::EncodingError(_))),
        "expected EncodingError for unsealed anf_ir_hash, got {result:?}"
    );
}

// Scenario: wasm_hash is sealed after emit_wasm.
#[test]
fn emit_wasm_seals_wasm_hash() {
    let anf = anf_for_n(1);
    let artifact = emit_wasm(&anf).unwrap();
    assert!(
        artifact.hash_chain.wasm_hash.is_some(),
        "wasm_hash must be Some after emit_wasm"
    );
}

// TRIANGULATE: different inputs produce different wasm hashes.
#[test]
fn different_anf_produces_different_wasm_hash() {
    let a1 = emit_wasm(&anf_for_n(1)).unwrap();
    let a2 = emit_wasm(&anf_for_n(2)).unwrap();
    assert_ne!(
        a1.hash_chain.wasm_hash, a2.hash_chain.wasm_hash,
        "different AnfIr inputs must produce different wasm_hashes"
    );
}

// Scenario: build_type_section returns None for 0 functions.
#[test]
fn build_type_section_none_for_zero() {
    assert!(build_type_section(&[]).is_none());
}

// TRIANGULATE: build_type_section returns Some for N > 0.
#[test]
fn build_type_section_some_for_nonzero() {
    let signature = WasmSignature {
        param_count: 0,
        result: None,
    };
    assert!(build_type_section(std::slice::from_ref(&signature)).is_some());
    assert!(build_type_section(&vec![signature; 5]).is_some());
}

fn sealed_anf(bindings: Vec<AnfBinding>) -> AnfIr {
    AnfIr {
        schema_version: ANF_SCHEMA_VERSION,
        source_map: SourceMap::from_bindings(&bindings),
        bindings,
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

#[test]
fn emit_wasm_call_uses_resolved_function_index() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.answer".to_string(),
            expr: AnfExpr::Literal(LiteralValue::Int(42)),
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn.main".to_string(),
            expr: AnfExpr::Call {
                func: "answer".to_string(),
                args: vec![],
            },
        },
    ]);

    let artifact = emit_wasm(&anf).unwrap();
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    let mut saw_call_answer = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if matches!(reader.read().unwrap(), Operator::Call { function_index: 0 }) {
                    saw_call_answer = true;
                }
            }
        }
    }

    assert!(saw_call_answer, "expected fn.main to call function index 0");
}

#[test]
fn emit_wasm_single_arg_call_emits_i64_add_and_call() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.double".to_string(),
            expr: AnfExpr::Call {
                func: "i64.add".to_string(),
                args: vec!["x".to_string(), "x".to_string()],
            },
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn.main".to_string(),
            expr: AnfExpr::Let {
                name: "n".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(21))),
                body: Box::new(AnfExpr::Call {
                    func: "double".to_string(),
                    args: vec!["n".to_string()],
                }),
            },
        },
    ]);

    let artifact = emit_wasm(&anf).unwrap();
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    let mut saw_i64_add = false;
    let mut saw_call_double = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                match reader.read().unwrap() {
                    Operator::I64Add => saw_i64_add = true,
                    Operator::Call { function_index: 0 } => saw_call_double = true,
                    _ => {}
                }
            }
        }
    }

    assert!(saw_i64_add, "expected double to use i64.add");
    assert!(saw_call_double, "expected main to call double");
}

#[test]
fn emit_wasm_multi_arg_call_emits_call() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.sum".to_string(),
            expr: AnfExpr::Call {
                func: "i64.add".to_string(),
                args: vec!["a".to_string(), "b".to_string()],
            },
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn.main".to_string(),
            expr: AnfExpr::Let {
                name: "a".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(20))),
                body: Box::new(AnfExpr::Let {
                    name: "b".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(22))),
                    body: Box::new(AnfExpr::Call {
                        func: "sum".to_string(),
                        args: vec!["a".to_string(), "b".to_string()],
                    }),
                }),
            },
        },
    ]);

    let artifact = emit_wasm(&anf).unwrap();
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    let mut saw_call_sum = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if matches!(reader.read().unwrap(), Operator::Call { function_index: 0 }) {
                    saw_call_sum = true;
                }
            }
        }
    }

    assert!(saw_call_sum, "expected main to call sum");
}

#[test]
fn emit_wasm_recursive_call_validates() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.recur".to_string(),
        expr: AnfExpr::Call {
            func: "recur".to_string(),
            args: vec!["n".to_string()],
        },
    }]);

    let artifact = emit_wasm(&anf).unwrap();
    wasmparser::validate(&artifact.wasm).expect("recursive call module must validate");

    let mut saw_self_call = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if matches!(reader.read().unwrap(), Operator::Call { function_index: 0 }) {
                    saw_self_call = true;
                }
            }
        }
    }

    assert!(
        saw_self_call,
        "recursive call should target its own function index"
    );
}

#[test]
fn emit_wasm_exports_literal_function_name() {
    use wasmparser::{ExternalKind, Parser, Payload};

    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.answer".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Int(42)),
    };
    let anf = AnfIr {
        schema_version: ANF_SCHEMA_VERSION,
        bindings: vec![binding.clone()],
        source_map: SourceMap::from_bindings(&[binding]),
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
    };

    let artifact = emit_wasm(&anf).unwrap();
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    let mut found = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::ExportSection(exports) = payload.unwrap() {
            for export in exports {
                let export = export.unwrap();
                if export.name == "answer" && export.kind == ExternalKind::Func {
                    found = true;
                }
            }
        }
    }

    assert!(found, "expected function export named answer");
}

#[test]
fn emit_wasm_record_new_and_field_get_use_linear_memory() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "rec".to_string(),
            value: Box::new(AnfExpr::RecordNew {
                fields: vec![
                    ("a".to_string(), AnfExpr::Literal(LiteralValue::Int(10))),
                    ("b".to_string(), AnfExpr::Literal(LiteralValue::Int(32))),
                ],
            }),
            body: Box::new(AnfExpr::FieldGet {
                record: "rec".to_string(),
                field: "b".to_string(),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).unwrap();
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    let mut saw_memory = false;
    let mut saw_store_b = false;
    let mut saw_load_b = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        match payload.unwrap() {
            Payload::MemorySection(_) => saw_memory = true,
            Payload::CodeSectionEntry(body) => {
                let mut reader = body.get_operators_reader().unwrap();
                while !reader.eof() {
                    match reader.read().unwrap() {
                        Operator::I64Store { memarg } if memarg.offset == 8 => saw_store_b = true,
                        Operator::I64Load { memarg } if memarg.offset == 8 => saw_load_b = true,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    assert!(saw_memory, "record codegen must declare linear memory");
    assert!(
        saw_store_b,
        "record construction must store field b at offset 8"
    );
    assert!(saw_load_b, "field get must load field b from offset 8");
}

#[test]
fn emit_wasm_tuple_list_variant_constructors_store_payloads() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.tuple".to_string(),
            expr: AnfExpr::TupleNew(vec![
                AnfExpr::Literal(LiteralValue::Int(1)),
                AnfExpr::Literal(LiteralValue::Int(2)),
            ]),
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn.list".to_string(),
            expr: AnfExpr::ListNew(vec![
                AnfExpr::Literal(LiteralValue::Int(3)),
                AnfExpr::Literal(LiteralValue::Int(4)),
            ]),
        },
        AnfBinding {
            source_ref: NodeRef(2),
            name: "fn.variant".to_string(),
            expr: AnfExpr::VariantNew {
                tag: "Some".to_string(),
                payload: Some(Box::new(AnfExpr::Literal(LiteralValue::Int(5)))),
            },
        },
    ]);

    let artifact = emit_wasm(&anf).unwrap();
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    let mut saw_tag_store = false;
    let mut i64_store_count = 0usize;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                match reader.read().unwrap() {
                    Operator::I32Store { .. } => saw_tag_store = true,
                    Operator::I64Store { .. } => i64_store_count += 1,
                    _ => {}
                }
            }
        }
    }

    assert!(
        saw_tag_store,
        "variant construction must store a tag discriminant (I32Store)"
    );
    assert!(
        i64_store_count >= 6,
        "tuple/list/variant constructors must store i64 payloads"
    );
}

// ── TASK-A3: stable VariantNew discriminant tests (TDD RED) ──────────
// Spec scenarios C-2a, C-2b, C-2c.

fn emit_two_variant_anf(tag_a: &str, tag_b: &str) -> AnfIr {
    // One function body with two sequential VariantNew lets.
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.variants".to_string(),
        expr: AnfExpr::Let {
            name: "v1".to_string(),
            value: Box::new(AnfExpr::VariantNew {
                tag: tag_a.to_string(),
                payload: None,
            }),
            body: Box::new(AnfExpr::Let {
                name: "v2".to_string(),
                value: Box::new(AnfExpr::VariantNew {
                    tag: tag_b.to_string(),
                    payload: None,
                }),
                body: Box::new(AnfExpr::Var("v1".to_string())),
            }),
        },
    };
    sealed_anf(vec![binding])
}

/// Extract all I32Const values seen in the code section of a WASM binary.
fn i32_const_values_in_code(wasm: &[u8]) -> Vec<i32> {
    use wasmparser::{Operator, Parser, Payload};
    let mut values = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::I32Const { value } = reader.read().unwrap() {
                    values.push(value);
                }
            }
        }
    }
    values
}

// ── TASK-A7: EffectCall I32 arg zero-extension tests (TDD RED) ───────
// Spec scenarios C-4a, C-4b.

fn emit_effect_call_with_i32_arg_wasm() -> Vec<u8> {
    // Let "rec" = VariantNew (I32) in EffectCall { cap: "test", args: ["rec"] }
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.effect_call_i32".to_string(),
        expr: AnfExpr::Let {
            name: "rec".to_string(),
            value: Box::new(AnfExpr::VariantNew {
                tag: "Tag".to_string(),
                payload: None,
            }),
            body: Box::new(AnfExpr::EffectCall {
                capability: "test.cap".to_string(),
                func: "op".to_string(),
                args: vec!["rec".to_string()],
            }),
        },
    };
    // Note: before A8 the WASM is invalid (I32 stored where I64 is needed).
    // We emit without validation here so we can inspect the instructions.
    emit_wasm(&sealed_anf(vec![binding]))
        .expect("emit_wasm must succeed")
        .wasm
}

// C-4a: I32 arg to EffectCall must be zero-extended (I64ExtendI32U emitted).
// Before A8: the WASM is either invalid OR missing I64ExtendI32U.
// After A8: WASM validates AND has I64ExtendI32U → I64Store sequence.
#[test]
fn effect_call_i32_arg_emits_i64_extend_before_store() {
    use wasmparser::{Operator, Parser, Payload};

    let wasm = emit_effect_call_with_i32_arg_wasm();

    // First: assert the WASM is valid (after A8 this must pass).
    wasmparser::validate(&wasm).expect("EffectCall with I32 arg must produce valid WASM");

    let mut saw_extend = false;
    let mut extend_before_store = false;

    for payload in Parser::new(0).parse_all(&wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                match reader.read().unwrap() {
                    Operator::I64ExtendI32U => {
                        saw_extend = true;
                    }
                    Operator::I64Store { .. } if saw_extend => {
                        extend_before_store = true;
                    }
                    _ => {}
                }
            }
        }
    }

    assert!(
        extend_before_store,
        "EffectCall with I32 arg must emit I64ExtendI32U before I64Store"
    );
}

// C-4b: I64 arg to EffectCall must NOT emit I64ExtendI32U (already 64-bit).
#[test]
fn effect_call_i64_arg_does_not_emit_extend() {
    use wasmparser::{Operator, Parser, Payload};

    // Let "n" = Int(42) (I64) in EffectCall { args: ["n"] }
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.effect_call_i64".to_string(),
        expr: AnfExpr::Let {
            name: "n".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(42))),
            body: Box::new(AnfExpr::EffectCall {
                capability: "test.cap".to_string(),
                func: "op".to_string(),
                args: vec!["n".to_string()],
            }),
        },
    };
    let artifact = emit_wasm(&sealed_anf(vec![binding])).expect("emit_wasm");
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    let mut extend_count = 0usize;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::I64ExtendI32U = reader.read().unwrap() {
                    extend_count += 1;
                }
            }
        }
    }

    assert_eq!(
        extend_count, 0,
        "EffectCall with I64 arg must NOT emit I64ExtendI32U (got {extend_count})"
    );
}

// C-2a: Different tag names produce different discriminants.
#[test]
fn variant_tag_ok_and_err_produce_different_discriminants() {
    let anf = emit_two_variant_anf("Ok", "Err");
    let artifact = emit_wasm(&anf).unwrap();
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    let consts = i32_const_values_in_code(&artifact.wasm);
    // There must be at least two I32Const values (one per VariantNew).
    assert!(
        consts.len() >= 2,
        "must have at least two i32.const (one per variant tag), got: {consts:?}"
    );
    // The two tag discriminants must differ.
    let first = consts[0];
    let second = consts.iter().find(|&&v| v != first);
    assert!(
        second.is_some(),
        "Ok and Err must produce different discriminants, got all equal: {consts:?}"
    );
}

// C-2b: Same tag name always produces the same discriminant.
// Verified by emitting the same single-variant binding twice and asserting
// that the resulting WASM bytes are byte-identical (deterministic discriminant).
#[test]
fn same_tag_name_produces_same_discriminant_across_calls() {
    let make_anf = || {
        let binding = AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.v".to_string(),
            expr: AnfExpr::VariantNew {
                tag: "Some".to_string(),
                payload: None,
            },
        };
        sealed_anf(vec![binding])
    };
    let art1 = emit_wasm(&make_anf()).unwrap();
    let art2 = emit_wasm(&make_anf()).unwrap();
    wasmparser::validate(&art1.wasm).expect("wasm1 must validate");
    wasmparser::validate(&art2.wasm).expect("wasm2 must validate");
    assert_eq!(
        art1.wasm, art2.wasm,
        "same AnfIr must produce byte-identical WASM (stable discriminant)"
    );
}

// ── TASK-A5: RuntimeCheck conditional trap tests (TDD RED) ───────────
// Spec scenarios C-3a, C-3b.
// These tests are structural (wasmparser) — they verify the emitted WASM
// instruction sequence for RuntimeCheck without requiring runtime execution.

fn emit_runtime_check_wasm(cond_name: &str) -> Vec<u8> {
    // Let "ok" = Int(1); RuntimeCheck { cond: "ok", .. }
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.guarded".to_string(),
        expr: AnfExpr::Let {
            name: cond_name.to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::RuntimeCheck {
                check_ref: "rtcheck.test".to_string(),
                cond: cond_name.to_string(),
                msg: "check failed".to_string(),
            }),
        },
    };
    let artifact = emit_wasm(&sealed_anf(vec![binding])).expect("emit_wasm");
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");
    artifact.wasm
}

// C-3a: RuntimeCheck emits a conditional trap (If+Unreachable+End),
// not an unconditional Unreachable.
// This test is RED with the current unconditional-Unreachable implementation.
#[test]
fn runtime_check_emits_conditional_trap_not_unconditional() {
    use wasmparser::{Operator, Parser, Payload};

    let wasm = emit_runtime_check_wasm("ok");

    let mut saw_if = false;
    let mut saw_unreachable_in_if = false;
    let mut in_if_block = false;

    for payload in Parser::new(0).parse_all(&wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                match reader.read().unwrap() {
                    Operator::If { .. } => {
                        saw_if = true;
                        in_if_block = true;
                    }
                    Operator::Unreachable if in_if_block => {
                        saw_unreachable_in_if = true;
                    }
                    Operator::End if in_if_block => {
                        in_if_block = false;
                    }
                    _ => {}
                }
            }
        }
    }

    assert!(
        saw_if,
        "RuntimeCheck must emit an If instruction for the conditional trap"
    );
    assert!(
        saw_unreachable_in_if,
        "RuntimeCheck must emit Unreachable inside an If block"
    );
}

// C-3b: A RuntimeCheck-returning function must NOT be exported
// (binding_result returns None for RuntimeCheck → not exported).
#[test]
fn runtime_check_function_is_not_exported() {
    use wasmparser::{ExternalKind, Parser, Payload};

    let wasm = emit_runtime_check_wasm("ok");

    let mut export_names: Vec<String> = Vec::new();
    for payload in Parser::new(0).parse_all(&wasm) {
        if let Payload::ExportSection(exports) = payload.unwrap() {
            for export in exports {
                let e = export.unwrap();
                if e.kind == ExternalKind::Func {
                    export_names.push(e.name.to_string());
                }
            }
        }
    }

    // RuntimeCheck returns None → binding_result returns None → not exported.
    assert!(
        !export_names.contains(&"guarded".to_string()),
        "RuntimeCheck-only function must not be exported (returns no value); exports: {export_names:?}"
    );
}

// ── TASK-A1: WasmTypeDescriptor + derive_wasm_type tests (TDD RED) ──
// These tests reference types/functions that don't exist yet.

#[test]
fn wasm_type_record_has_field_names() {
    let expr = AnfExpr::RecordNew {
        fields: vec![
            ("x".to_string(), AnfExpr::Literal(LiteralValue::Int(1))),
            ("y".to_string(), AnfExpr::Literal(LiteralValue::Int(2))),
        ],
    };
    let ty = derive_wasm_type(&expr);
    assert_eq!(
        ty,
        WasmTypeDescriptor::Record {
            fields: vec!["x".to_string(), "y".to_string()]
        }
    );
}

#[test]
fn wasm_type_variant_has_tag() {
    let expr = AnfExpr::VariantNew {
        tag: "Ok".to_string(),
        payload: Some(Box::new(AnfExpr::Literal(LiteralValue::Int(1)))),
    };
    let ty = derive_wasm_type(&expr);
    assert_eq!(
        ty,
        WasmTypeDescriptor::Variant {
            tags: vec!["Ok".to_string()]
        }
    );
}

#[test]
fn wasm_type_int_literal_is_scalar_i64() {
    let expr = AnfExpr::Literal(LiteralValue::Int(1));
    let ty = derive_wasm_type(&expr);
    assert_eq!(ty, WasmTypeDescriptor::Scalar(WasmScalarType::I64));
}

#[test]
fn wasm_type_let_body_propagates() {
    let expr = AnfExpr::Let {
        name: "r".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
        body: Box::new(AnfExpr::RecordNew {
            fields: vec![("a".to_string(), AnfExpr::Literal(LiteralValue::Int(1)))],
        }),
    };
    let ty = derive_wasm_type(&expr);
    assert_eq!(
        ty,
        WasmTypeDescriptor::Record {
            fields: vec!["a".to_string()]
        }
    );
}

#[test]
fn wasm_type_list_new_is_list() {
    let expr = AnfExpr::ListNew(vec![AnfExpr::Literal(LiteralValue::Int(1))]);
    let ty = derive_wasm_type(&expr);
    assert_eq!(
        ty,
        WasmTypeDescriptor::List(Box::new(WasmTypeDescriptor::Scalar(WasmScalarType::I64)))
    );
}

#[test]
fn wasm_type_tuple_new_is_tuple_in_declaration_order() {
    let expr = AnfExpr::TupleNew(vec![
        AnfExpr::Literal(LiteralValue::Int(1)),
        AnfExpr::Literal(LiteralValue::Unit),
    ]);
    let ty = derive_wasm_type(&expr);
    assert_eq!(
        ty,
        WasmTypeDescriptor::Tuple(vec![
            WasmTypeDescriptor::Scalar(WasmScalarType::I64),
            WasmTypeDescriptor::Scalar(WasmScalarType::I32),
        ])
    );
}

// ── TASK-A3: WasmArtifact.export_types tests (TDD RED) ───────────────

#[test]
fn emit_wasm_record_function_is_exported() {
    use wasmparser::{ExternalKind, Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.make_pair".to_string(),
        expr: AnfExpr::RecordNew {
            fields: vec![
                ("x".to_string(), AnfExpr::Literal(LiteralValue::Int(1))),
                ("y".to_string(), AnfExpr::Literal(LiteralValue::Int(2))),
            ],
        },
    }]);

    let artifact = emit_wasm(&anf).unwrap();
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    // The function must be exported.
    let mut found_export = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::ExportSection(exports) = payload.unwrap() {
            for export in exports {
                let e = export.unwrap();
                if e.name == "make_pair" && e.kind == ExternalKind::Func {
                    found_export = true;
                }
            }
        }
    }
    assert!(
        found_export,
        "RecordNew binding must be exported as 'make_pair'"
    );

    // export_types must contain Record descriptor for this binding.
    assert!(
        artifact.export_types.contains_key("make_pair"),
        "export_types must contain 'make_pair'"
    );
    assert_eq!(
        artifact.export_types["make_pair"],
        WasmTypeDescriptor::Record {
            fields: vec!["x".to_string(), "y".to_string()]
        }
    );
}

#[test]
fn emit_wasm_export_types_has_scalar_for_int() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.answer".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Int(42)),
    }]);

    let artifact = emit_wasm(&anf).unwrap();
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    assert_eq!(
        artifact.export_types.get("answer"),
        Some(&WasmTypeDescriptor::Scalar(WasmScalarType::I64))
    );
}

#[test]
fn emit_wasm_export_types_has_record_with_fields() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.rec".to_string(),
        expr: AnfExpr::RecordNew {
            fields: vec![
                ("a".to_string(), AnfExpr::Literal(LiteralValue::Int(10))),
                ("b".to_string(), AnfExpr::Literal(LiteralValue::Int(20))),
            ],
        },
    }]);

    let artifact = emit_wasm(&anf).unwrap();
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    assert_eq!(
        artifact.export_types.get("rec"),
        Some(&WasmTypeDescriptor::Record {
            fields: vec!["a".to_string(), "b".to_string()]
        })
    );
}

#[test]
fn emit_wasm_variant_function_is_exported() {
    use wasmparser::{ExternalKind, Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.make_variant".to_string(),
        expr: AnfExpr::VariantNew {
            tag: "Ok".to_string(),
            payload: Some(Box::new(AnfExpr::Literal(LiteralValue::Int(5)))),
        },
    }]);

    let artifact = emit_wasm(&anf).unwrap();
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    let mut found_export = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::ExportSection(exports) = payload.unwrap() {
            for export in exports {
                let e = export.unwrap();
                if e.name == "make_variant" && e.kind == ExternalKind::Func {
                    found_export = true;
                }
            }
        }
    }
    assert!(found_export, "VariantNew binding must be exported");
    assert!(
        artifact.export_types.contains_key("make_variant"),
        "export_types must contain 'make_variant'"
    );
    assert_eq!(
        artifact.export_types["make_variant"],
        WasmTypeDescriptor::Variant {
            tags: vec!["Ok".to_string()]
        }
    );
}

// C-2c: Tag discriminant is stored as a full i32 (not i8) at offset 0.
// This test is RED with the current I32Store8 implementation.
#[test]
fn variant_discriminant_stored_as_i32_at_offset_0() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = emit_two_variant_anf("Tag", "Tag");
    let artifact = emit_wasm(&anf).unwrap();
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    let mut saw_i32_store_at_0 = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::I32Store { memarg } = reader.read().unwrap()
                    && memarg.offset == 0
                {
                    saw_i32_store_at_0 = true;
                }
            }
        }
    }

    assert!(
        saw_i32_store_at_0,
        "VariantNew tag must be stored as a full i32 (I32Store at offset 0), not I32Store8"
    );
}

// ── TASK-E1: host_call_write codegen tests (TDD RED) ─────────────────
// These tests verify that when an EffectCall result flows into a structured
// context (RecordNew), the emitted WASM:
//   1. Imports "ail"/"host_call_write".
//   2. EffectDataLayout has result_buffer_offset > args_offset.
//   3. The code section contains a Call to function index 1 (host_call_write).

fn effect_call_with_record_result_anf() -> AnfIr {
    // let effect_result = effect_call("data", "fetch", []);
    // record_new([("val", effect_result)])
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.fetch_record".to_string(),
        expr: AnfExpr::Let {
            name: "effect_result".to_string(),
            value: Box::new(AnfExpr::EffectCall {
                capability: "data".to_string(),
                func: "fetch".to_string(),
                args: vec![],
            }),
            body: Box::new(AnfExpr::RecordNew {
                fields: vec![("val".to_string(), AnfExpr::Var("effect_result".to_string()))],
            }),
        },
    };
    sealed_anf(vec![binding])
}

#[test]
fn effect_call_structured_return_emits_host_call_write_import() {
    use wasmparser::{Parser, Payload};

    let anf = effect_call_with_record_result_anf();
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    let mut found_host_call_write = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::ImportSection(imports) = payload.unwrap() {
            for imp in imports.into_imports() {
                let imp = imp.unwrap();
                if imp.module == "ail" && imp.name == "host_call_write" {
                    found_host_call_write = true;
                }
            }
        }
    }

    assert!(
        found_host_call_write,
        "structured EffectCall must import 'ail'/'host_call_write'"
    );
}

#[test]
fn effect_data_layout_has_result_buffer_offset() {
    let anf = effect_call_with_record_result_anf();
    let layout = EffectDataLayout::for_bindings(&anf.bindings);

    assert!(
        layout.needs_host_call_write,
        "EffectDataLayout must set needs_host_call_write for structured EffectCall"
    );
    assert!(
        layout.result_buffer_offset > layout.args_offset,
        "result_buffer_offset ({}) must be greater than args_offset ({})",
        layout.result_buffer_offset,
        layout.args_offset,
    );
}

#[test]
fn host_call_write_call_passes_out_ptr() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = effect_call_with_record_result_anf();
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    // host_call_write is imported as function index 1 (after host_call at 0).
    let mut saw_call_1 = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::Call { function_index: 1 } = reader.read().unwrap() {
                    saw_call_1 = true;
                }
            }
        }
    }

    assert!(
        saw_call_1,
        "structured EffectCall must emit Call {{ function_index: 1 }} (host_call_write)"
    );
}

// ── derive_wasm_type EffectCall limitation tests ──────────────────────────
//
// LIMITATION: `derive_wasm_type` always returns `Scalar(I64)` for an
// `EffectCall` node because:
//   - ANF expressions carry no return-type annotation at this stage.
//   - There are no handler descriptors available to look up the declared
//     return type of the capability operation.
//
// This is intentional and explicitly documented here so future implementors
// know what to fix: either propagate return-type annotations from the
// type-checker into ANF, or pass a handler-descriptor table into
// `derive_wasm_type`.

// Scenario: bare EffectCall derives Scalar(I64).
// Proves the explicit arm fires and the fallback wildcard is not relied on.
#[test]
fn derive_wasm_type_effect_call_is_scalar_i64() {
    let expr = AnfExpr::EffectCall {
        capability: "test.cap".to_string(),
        func: "op".to_string(),
        args: vec![],
    };
    assert_eq!(
        derive_wasm_type(&expr),
        WasmTypeDescriptor::Scalar(WasmScalarType::I64),
        "EffectCall must derive Scalar(I64): no ANF return-type annotation available"
    );
}

// Scenario: Let { body: EffectCall } also derives Scalar(I64).
// The Let arm recurses into `body`; the EffectCall arm then fires.
// Documents that the limitation persists through nested Let bindings.
#[test]
fn derive_wasm_type_let_body_effect_call_is_scalar_i64() {
    let expr = AnfExpr::Let {
        name: "result".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
        body: Box::new(AnfExpr::EffectCall {
            capability: "io".to_string(),
            func: "read".to_string(),
            args: vec![],
        }),
    };
    assert_eq!(
        derive_wasm_type(&expr),
        WasmTypeDescriptor::Scalar(WasmScalarType::I64),
        "Let body EffectCall must derive Scalar(I64): limitation applies through Let nesting"
    );
}

// ── collect_free_vars: EffectCall args ────────────────────────────────────

// Scenario: EffectCall args that are not locally bound are collected as free vars.
// Proves the gap fixed: EffectCall previously fell through to `_ => {}` in
// collect_free_vars, silently dropping its arg references from binding_params.
#[test]
fn collect_free_vars_effect_call_args_are_included() {
    // Let "x" = 1 in EffectCall { args: ["x", "y"] }
    // "x" is bound by the Let so it must NOT appear in free vars.
    // "y" is free — it must appear.
    let expr = AnfExpr::Let {
        name: "x".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        body: Box::new(AnfExpr::EffectCall {
            capability: "io".to_string(),
            func: "write".to_string(),
            args: vec!["x".to_string(), "y".to_string()],
        }),
    };
    let mut bound = vec![];
    let mut out = vec![];
    collect_free_vars(&expr, &mut bound, &mut out);
    assert!(
        !out.contains(&"x"),
        "bound var 'x' must not appear in free vars; got: {out:?}"
    );
    assert!(
        out.contains(&"y"),
        "free var 'y' must appear in free vars; got: {out:?}"
    );
}

// Scenario: binding_params reports EffectCall args as parameters.
// binding_params is the pub(crate) path consumed by binding_signatures.
// A bare EffectCall binding with two args must produce param_count == 2.
#[test]
fn binding_params_includes_effect_call_args() {
    let binding = AnfBinding {
        name: "fn_effect".to_string(),
        source_ref: NodeRef(0),
        expr: AnfExpr::EffectCall {
            capability: "cap".to_string(),
            func: "op".to_string(),
            args: vec!["a".to_string(), "b".to_string()],
        },
    };
    let params = binding_params(&binding);
    assert_eq!(
        params.len(),
        2,
        "binding_params must include both EffectCall args; got: {params:?}"
    );
    assert!(
        params.contains(&"a"),
        "param 'a' must be present; got: {params:?}"
    );
    assert!(
        params.contains(&"b"),
        "param 'b' must be present; got: {params:?}"
    );
}

// ── Feature-H: WASM capability manifest ──────────────────────────────────

// Scenario: WasmArtifact carries a capabilities_manifest with one entry per binding.
// Spec: "capabilities_manifest.entries.len() == N bindings"
#[test]
fn emit_wasm_capabilities_manifest_len_equals_binding_count() {
    for n in [0usize, 1, 3, 5] {
        let anf = anf_for_n(n);
        let artifact = emit_wasm(&anf).unwrap();
        assert_eq!(
            artifact.capabilities_manifest.entries.len(),
            n,
            "capabilities_manifest must have {n} entries for {n}-binding AnfIr"
        );
    }
}

// Scenario: empty AnfIr produces empty capabilities_manifest.
// Triangulate: zero bindings → zero entries.
#[test]
fn emit_wasm_empty_anf_produces_empty_capabilities_manifest() {
    let anf = anf_for_n(0);
    let artifact = emit_wasm(&anf).unwrap();
    assert!(
        artifact.capabilities_manifest.entries.is_empty(),
        "empty AnfIr must produce empty capabilities_manifest"
    );
}

// Scenario: capabilities_manifest entries carry correct names and source_refs.
// Spec: "entry.name == binding.name; entry.source_ref == binding.source_ref"
#[test]
fn emit_wasm_capabilities_manifest_entries_match_bindings() {
    let n = 3usize;
    let anf = anf_for_n(n);
    let artifact = emit_wasm(&anf).unwrap();
    for (i, entry) in artifact.capabilities_manifest.entries.iter().enumerate() {
        assert_eq!(
            entry.name,
            format!("fn_{i}"),
            "entry {i} name must match binding name"
        );
        assert_eq!(
            entry.source_ref,
            ail_core::semantic_graph::NodeRef(i as u32),
            "entry {i} source_ref must match binding source_ref"
        );
    }
}

// Scenario: capabilities_manifest_hash in artifact_manifest is derived from
// the real manifest bytes, not a proxy over raw bindings.
// Triangulate: two different N-binding AnfIrs produce different manifest hashes.
#[test]
fn emit_wasm_different_bindings_produce_different_capabilities_manifest_hash() {
    let a1 = emit_wasm(&anf_for_n(1)).unwrap();
    let a2 = emit_wasm(&anf_for_n(2)).unwrap();
    assert_ne!(
        a1.artifact_manifest.capabilities_manifest_hash,
        a2.artifact_manifest.capabilities_manifest_hash,
        "different binding counts must produce different capabilities_manifest_hash"
    );
}

// Scenario: capabilities_manifest serialises to JSON with entries array.
// Spec: JSON consumers must be able to parse capabilities_manifest.entries uniformly.
#[test]
fn emit_wasm_capabilities_manifest_serialises_to_json_with_entries_array() {
    let anf = anf_for_n(2);
    let artifact = emit_wasm(&anf).unwrap();
    let json_val = serde_json::to_value(&artifact.capabilities_manifest)
        .expect("capabilities_manifest must serialise to JSON");
    assert!(
        json_val["entries"].is_array(),
        "serialised capabilities_manifest must have entries array; got: {json_val}"
    );
    assert_eq!(
        json_val["entries"].as_array().unwrap().len(),
        2,
        "entries array must have 2 elements for 2-binding AnfIr"
    );
}
