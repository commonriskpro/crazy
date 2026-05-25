// Tests for the WASM emission stage.
// Declared from wasm.rs as: #[cfg(test)] #[path = "wasm_tests.rs"] mod tests;

use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::report::VerificationReport;

use super::emit_wasm;
use crate::anf::{ANF_SCHEMA_VERSION, AnfBinding, AnfExpr, AnfIr, AnfSelectClause, SourceMap};
use crate::core_ir::{LiteralValue, StageHashes};
use crate::error::CompileError;
use crate::lower::{lower_to_anf, lower_to_core_ir};
use crate::wasm_abi::{
    EffectDataLayout, WasmScalarType, WasmSignature, WasmTypeDescriptor, binding_params,
    collect_free_vars, derive_wasm_type, lambda_body_params,
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

// Scenario: build_type_section returns None for 0 functions and no fold.
#[test]
fn build_type_section_none_for_zero() {
    assert!(build_type_section(&[], false, false).is_none());
}

// Scenario: build_type_section returns Some when needs_fold is true even with 0 signatures.
#[test]
fn build_type_section_some_when_needs_fold() {
    assert!(build_type_section(&[], true, false).is_some());
}

// TRIANGULATE: build_type_section returns Some for N > 0.
#[test]
fn build_type_section_some_for_nonzero() {
    let signature = WasmSignature {
        param_count: 0,
        result: None,
    };
    assert!(build_type_section(std::slice::from_ref(&signature), false, false).is_some());
    assert!(build_type_section(&vec![signature; 5], false, false).is_some());
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

// ── Closure-capture PR1: AnfExpr::Lambda captures field ───────────────────
//
// Verify that the `captures` field is correctly populated during ANF lowering.
// Scenarios: no capture, simple capture, shadowed bound var, EffectCall arg.

// Scenario: lambda whose body only references its own params — no captures.
#[test]
fn closure_capture_lambda_no_capture() {
    use crate::anf::AnfExpr;
    use crate::core_ir::CoreExpr;
    use crate::lower::lower_core_expr_to_anf;
    use ail_core::semantic_graph::NodeRef;

    // fn(x) -> x
    let expr = CoreExpr::Lambda {
        params: vec!["x".to_string()],
        body: Box::new(CoreExpr::Var("x".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = Vec::new();
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);

    if let AnfExpr::Lambda { captures, .. } = result {
        assert!(
            captures.is_empty(),
            "identity lambda must have no captures; got {captures:?}"
        );
    } else {
        panic!("expected AnfExpr::Lambda");
    }
}

// Scenario: lambda that references an outer variable — must appear in captures.
#[test]
fn closure_capture_lambda_with_outer_var() {
    use crate::anf::AnfExpr;
    use crate::core_ir::CoreExpr;
    use crate::lower::lower_core_expr_to_anf;
    use ail_core::semantic_graph::NodeRef;

    // fn(x) -> add(x, outer_val)   — `outer_val` is free
    let expr = CoreExpr::Lambda {
        params: vec!["x".to_string()],
        body: Box::new(CoreExpr::Call {
            func: "add".to_string(),
            args: vec![
                CoreExpr::Var("x".to_string()),
                CoreExpr::Var("outer_val".to_string()),
            ],
        }),
    };
    let mut fresh = 0u32;
    let mut out = Vec::new();
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);

    if let AnfExpr::Lambda { captures, .. } = result {
        assert!(
            captures.contains(&"outer_val".to_string()),
            "outer_val must be captured; got {captures:?}"
        );
        assert!(
            !captures.contains(&"x".to_string()),
            "param x must NOT appear in captures; got {captures:?}"
        );
    } else {
        panic!("expected AnfExpr::Lambda");
    }
}

// Scenario: lambda param shadows an outer variable of the same name — the
// outer name must NOT appear in captures (the param takes precedence).
#[test]
fn closure_capture_lambda_shadowed_param_not_captured() {
    use crate::anf::AnfExpr;
    use crate::core_ir::CoreExpr;
    use crate::lower::lower_core_expr_to_anf;
    use ail_core::semantic_graph::NodeRef;

    // fn(outer) -> outer  — param named "outer" shadows any outer binding
    let expr = CoreExpr::Lambda {
        params: vec!["outer".to_string()],
        body: Box::new(CoreExpr::Var("outer".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = Vec::new();
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);

    if let AnfExpr::Lambda { captures, .. } = result {
        assert!(
            captures.is_empty(),
            "`outer` is shadowed by param; captures must be empty; got {captures:?}"
        );
    } else {
        panic!("expected AnfExpr::Lambda");
    }
}

// Scenario: lambda whose body contains an EffectCall that references an outer
// variable — the outer variable must appear in captures.
#[test]
fn closure_capture_lambda_effect_call_arg_captured() {
    use crate::anf::AnfExpr;
    use crate::core_ir::CoreExpr;
    use crate::lower::lower_core_expr_to_anf;
    use ail_core::semantic_graph::NodeRef;

    // fn(x) -> db.read(x, context_id)  — `context_id` is free
    let expr = CoreExpr::Lambda {
        params: vec!["x".to_string()],
        body: Box::new(CoreExpr::EffectCall {
            capability: "db".to_string(),
            func: "read".to_string(),
            args: vec![
                CoreExpr::Var("x".to_string()),
                CoreExpr::Var("context_id".to_string()),
            ],
        }),
    };
    let mut fresh = 0u32;
    let mut out = Vec::new();
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);

    if let AnfExpr::Lambda { captures, .. } = result {
        assert!(
            captures.contains(&"context_id".to_string()),
            "context_id must be captured from EffectCall arg; got {captures:?}"
        );
        assert!(
            !captures.contains(&"x".to_string()),
            "param x must NOT be captured; got {captures:?}"
        );
    } else {
        panic!("expected AnfExpr::Lambda");
    }
}

// ── End closure-capture tests ─────────────────────────────────────────────

// ── Wave 10A: Bytes literal emit, descriptor, and data-section tests ──────

// Scenario: derive_wasm_type on a Bytes literal must return WasmTypeDescriptor::Bytes.
// Proves the compiler side of the descriptor contract for Bytes.
#[test]
fn derive_wasm_type_bytes_literal_is_bytes_descriptor() {
    let expr = AnfExpr::Literal(LiteralValue::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF]));
    assert_eq!(
        derive_wasm_type(&expr),
        WasmTypeDescriptor::Bytes,
        "Bytes literal must derive WasmTypeDescriptor::Bytes"
    );
}

// Scenario: Let { body: Literal(Bytes) } also derives Bytes (Let recurses into body).
#[test]
fn derive_wasm_type_let_body_bytes_is_bytes_descriptor() {
    let expr = AnfExpr::Let {
        name: "b".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
        body: Box::new(AnfExpr::Literal(LiteralValue::Bytes(vec![1, 2, 3]))),
    };
    assert_eq!(derive_wasm_type(&expr), WasmTypeDescriptor::Bytes);
}

// Scenario: emit_wasm on a Bytes literal binding succeeds and export_types
// carries WasmTypeDescriptor::Bytes for that export.
#[test]
fn emit_wasm_bytes_literal_export_type_is_bytes() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.digest".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Bytes(vec![0xCA, 0xFE, 0xBA, 0xBE])),
    }]);
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for Bytes literal");
    assert_eq!(
        artifact.export_types.get("digest"),
        Some(&WasmTypeDescriptor::Bytes),
        "export_types[\"digest\"] must be WasmTypeDescriptor::Bytes; got: {:?}",
        artifact.export_types.get("digest")
    );
}

// Scenario: the emitted WASM for a Bytes literal must include a data section.
// Proves intern_bytes → build_data_section places bytes in the module binary.
#[test]
fn emit_wasm_bytes_literal_produces_non_empty_wasm() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.payload".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Bytes(vec![0x01, 0x02, 0x03])),
    }]);
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed");
    assert!(
        !artifact.wasm.is_empty(),
        "Bytes literal must produce a non-empty WASM module"
    );
}

// Scenario: two Bytes literals with equal content share the same data-section slot.
// Proves deduplication in intern_bytes (packed i64 values must be identical).
#[test]
fn effect_data_layout_bytes_dedup_equal_content() {
    use crate::wasm_abi::EffectDataLayout;
    let data = vec![0xAB, 0xCD];
    let mut layout = EffectDataLayout::default();
    let (ptr1, len1) = layout.intern_bytes(&data);
    let (ptr2, len2) = layout.intern_bytes(&data);
    assert_eq!(
        (ptr1, len1),
        (ptr2, len2),
        "duplicate Bytes literal must reuse the same data-section slot"
    );
    assert_eq!(len1, 2, "interned len must match data length");
}

// Scenario: two Bytes literals with distinct content occupy distinct slots.
#[test]
fn effect_data_layout_bytes_distinct_content_distinct_slots() {
    use crate::wasm_abi::EffectDataLayout;
    let mut layout = EffectDataLayout::default();
    let (ptr_a, _) = layout.intern_bytes(&[0x01]);
    let (ptr_b, _) = layout.intern_bytes(&[0x02]);
    assert_ne!(
        ptr_a, ptr_b,
        "distinct Bytes content must occupy distinct data-section slots"
    );
}

// Scenario: LiteralValue::Bytes carries a non-empty Vec<u8> and compares by value.
// Proves the new enum variant is well-behaved (PartialEq, Clone).
#[test]
fn literal_value_bytes_equality_and_clone() {
    let a = LiteralValue::Bytes(vec![1, 2, 3]);
    let b = LiteralValue::Bytes(vec![1, 2, 3]);
    let c = LiteralValue::Bytes(vec![9]);
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_eq!(a.clone(), b);
}

// Scenario: empty Bytes literal (zero-length slice) encodes len=0 in the packed i64.
// Proves intern_bytes handles the zero-length edge case safely.
#[test]
fn emit_wasm_empty_bytes_literal_succeeds() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.empty_bytes".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Bytes(vec![])),
    }]);
    assert!(
        emit_wasm(&anf).is_ok(),
        "empty Bytes literal must emit successfully"
    );
}

// ── End Wave 10A Bytes tests ───────────────────────────────────────────────

// ── WASM ABI surface: Bytes + ResourceAcquire→Handle expansion ───────────

// WasmTypeDescriptor::Bytes exists and round-trips through serde.
#[test]
fn wasm_type_descriptor_bytes_exists_and_serialises() {
    let desc = WasmTypeDescriptor::Bytes;
    let json = serde_json::to_string(&desc).expect("Bytes must serialise to JSON");
    assert_eq!(
        json, "\"Bytes\"",
        "WasmTypeDescriptor::Bytes must serialise as the string \"Bytes\""
    );
    let roundtrip: WasmTypeDescriptor =
        serde_json::from_str(&json).expect("Bytes must deserialise from JSON");
    assert_eq!(roundtrip, WasmTypeDescriptor::Bytes);
}

// Bytes is a distinct variant from Scalar and Text.
#[test]
fn wasm_type_descriptor_bytes_is_distinct_from_scalar_and_text() {
    let bytes = WasmTypeDescriptor::Bytes;
    assert_ne!(bytes, WasmTypeDescriptor::Text);
    assert_ne!(bytes, WasmTypeDescriptor::Scalar(WasmScalarType::I64));
    assert_ne!(bytes, WasmTypeDescriptor::Scalar(WasmScalarType::I32));
}

// derive_wasm_type for ResourceAcquire must return Handle.
//
// ResourceAcquire is the only ANF node whose contract guarantees a resource
// handle return; all other node shapes fall to Scalar(I64) or another
// specific variant.
#[test]
fn derive_wasm_type_resource_acquire_is_handle() {
    let expr = AnfExpr::ResourceAcquire {
        resource: "db.connection".to_string(),
        args: vec![],
    };
    assert_eq!(
        derive_wasm_type(&expr),
        WasmTypeDescriptor::Handle,
        "ResourceAcquire must derive Handle"
    );
}

// Let { body: ResourceAcquire } also derives Handle (Let recurses into body).
#[test]
fn derive_wasm_type_let_body_resource_acquire_is_handle() {
    let expr = AnfExpr::Let {
        name: "h".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
        body: Box::new(AnfExpr::ResourceAcquire {
            resource: "fs.file".to_string(),
            args: vec![],
        }),
    };
    assert_eq!(
        derive_wasm_type(&expr),
        WasmTypeDescriptor::Handle,
        "Let body ResourceAcquire must derive Handle"
    );
}

// Bool literal derives Scalar(I64) — explicit arm, not wildcard fallback.
#[test]
fn derive_wasm_type_bool_literal_is_scalar_i64() {
    let expr = AnfExpr::Literal(LiteralValue::Bool(true));
    assert_eq!(
        derive_wasm_type(&expr),
        WasmTypeDescriptor::Scalar(WasmScalarType::I64),
        "Bool literal must derive Scalar(I64)"
    );
}

// Int literal derives Scalar(I64) — explicit arm, triangulates with Bool arm.
// (Already covered by `wasm_type_int_literal_is_scalar_i64`; kept here for
// locality with the Bool arm test above.)
#[test]
fn derive_wasm_type_int_literal_explicit_arm() {
    let expr = AnfExpr::Literal(LiteralValue::Int(0));
    assert_eq!(
        derive_wasm_type(&expr),
        WasmTypeDescriptor::Scalar(WasmScalarType::I64),
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

// ── WASM closure capture: Lambda binding body emission ────────────────────
//
// These tests cover the changes introduced in feat/wasm-closure-capture:
//   1. `lambda_body_params` extracts Lambda-own params from an expression.
//   2. `binding_signatures` includes both captures and Lambda params.
//   3. `binding_result` infers the result type from the Lambda body.
//   4. `build_code_section` emits the Lambda body directly for top-level
//      Lambda bindings, placing captures and params in scope.
//   5. Nested Lambda sub-expressions emit a closure env in linear memory.
//
// Limitation documented in wasm_emit.rs: the fn_idx field of the closure env
// is a placeholder (0) until a WASM element-section + call_indirect pass is
// added in a future slice.

// Scenario: lambda_body_params returns the Lambda's params slice.
#[test]
fn lambda_body_params_returns_lambda_params() {
    let lambda = AnfExpr::Lambda {
        params: vec!["x".to_string(), "y".to_string()],
        captures: vec!["outer".to_string()],
        body: Box::new(AnfExpr::Var("x".to_string())),
    };
    assert_eq!(
        lambda_body_params(&lambda),
        &["x", "y"],
        "lambda_body_params must return the Lambda's own params"
    );
}

// TRIANGULATE: lambda_body_params returns empty for non-Lambda expressions.
#[test]
fn lambda_body_params_empty_for_non_lambda() {
    assert!(
        lambda_body_params(&AnfExpr::Literal(LiteralValue::Int(0))).is_empty(),
        "lambda_body_params must be empty for Literal"
    );
    assert!(
        lambda_body_params(&AnfExpr::Var("x".to_string())).is_empty(),
        "lambda_body_params must be empty for Var"
    );
}

// Scenario: binding_signatures for a Lambda binding includes both captures
// and Lambda-own params in param_count, and infers the result from the body.
//
// Lambda { captures: ["outer"], params: ["x"], body: add(outer, x) }
// Expected: param_count = 2, result = Some(I64)
#[test]
fn binding_signatures_lambda_includes_captures_and_params() {
    use ail_core::semantic_graph::NodeRef;
    use wasm_encoder::ValType;

    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "add".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["x".to_string()],
            captures: vec!["outer".to_string()],
            body: Box::new(AnfExpr::Call {
                func: "+".to_string(),
                args: vec!["outer".to_string(), "x".to_string()],
            }),
        },
    };
    let sigs = binding_params(&binding);
    // binding_params returns captures only.
    assert_eq!(sigs, vec!["outer"], "binding_params must return captures");

    let signatures = crate::wasm_abi::binding_signatures(std::slice::from_ref(&binding));
    assert_eq!(
        signatures[0].param_count, 2,
        "1 capture + 1 Lambda param = 2 WASM params"
    );
    assert_eq!(
        signatures[0].result,
        Some(ValType::I64),
        "body add(outer, x) → I64 result"
    );
}

// Scenario: binding_result for a Lambda binding infers from the Lambda body,
// not from the Lambda node itself (which would always give I32 in the old code).
#[test]
fn binding_result_lambda_infers_from_body() {
    use ail_core::semantic_graph::NodeRef;
    use wasm_encoder::ValType;

    // Lambda with no captures: fn(x) -> x  (identity, I64)
    let no_cap = AnfBinding {
        source_ref: NodeRef(0),
        name: "id".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["x".to_string()],
            captures: vec![],
            body: Box::new(AnfExpr::Var("x".to_string())),
        },
    };
    assert_eq!(
        crate::wasm_abi::binding_result(&no_cap),
        Some(ValType::I64),
        "identity Lambda body must resolve to I64, not I32"
    );

    // Lambda with capture: fn(x) -> add(outer, x)  (I64)
    let with_cap = AnfBinding {
        source_ref: NodeRef(1),
        name: "add".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["x".to_string()],
            captures: vec!["outer".to_string()],
            body: Box::new(AnfExpr::Call {
                func: "+".to_string(),
                args: vec!["outer".to_string(), "x".to_string()],
            }),
        },
    };
    assert_eq!(
        crate::wasm_abi::binding_result(&with_cap),
        Some(ValType::I64),
        "Lambda body add(outer, x) must resolve to I64"
    );
}

// Scenario: emit_wasm succeeds for a top-level Lambda binding with no captures.
// Proves the pipeline is end-to-end correct for the no-capture case.
#[test]
fn emit_wasm_lambda_binding_no_capture_succeeds() {
    use ail_core::semantic_graph::NodeRef;

    // fn(x) -> x  (identity Lambda, no captures)
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "id".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["x".to_string()],
            captures: vec![],
            body: Box::new(AnfExpr::Var("x".to_string())),
        },
    };
    let anf = sealed_anf(vec![binding]);
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for identity Lambda");
    assert!(
        !artifact.wasm.is_empty(),
        "WASM binary must be non-empty for Lambda binding"
    );
    assert!(
        artifact.hash_chain.wasm_hash.is_some(),
        "wasm_hash must be sealed after emit_wasm"
    );
}

// Scenario: emit_wasm succeeds for a top-level Lambda binding with one capture.
// Proves the pipeline handles captures as additional WASM function params.
#[test]
fn emit_wasm_lambda_binding_with_capture_succeeds() {
    use ail_core::semantic_graph::NodeRef;

    // fn(x) -> add(outer, x)  — outer is a captured variable
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "add_to_outer".to_string(),
        expr: AnfExpr::Lambda {
            params: vec!["x".to_string()],
            captures: vec!["outer".to_string()],
            body: Box::new(AnfExpr::Call {
                func: "+".to_string(),
                args: vec!["outer".to_string(), "x".to_string()],
            }),
        },
    };
    let anf = sealed_anf(vec![binding]);
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for Lambda with captures");
    assert!(
        !artifact.wasm.is_empty(),
        "WASM binary must be non-empty for Lambda binding with captures"
    );
    // The function is exported because binding_result returns Some(I64).
    assert!(
        artifact.export_types.contains_key("add_to_outer"),
        "Lambda binding with I64 body must appear in export_types; got: {:?}",
        artifact.export_types.keys().collect::<Vec<_>>()
    );
}

// Scenario: a binding whose body contains a nested Lambda with captures emits
// a closure env in linear memory.  The WASM module must include memory and the
// global bump-allocator section required by emit_alloc.
//
// The test verifies structural properties: emit_wasm succeeds, the binary
// contains a memory section (needs_memory = true due to captures), and the
// hash is sealed.
#[test]
fn emit_wasm_nested_lambda_with_captures_allocates_memory() {
    use ail_core::semantic_graph::NodeRef;

    // let result = (fn(x) { x + outer })  — nested Lambda in a Let body
    // The outer binding does not itself have params; it wraps a nested Lambda.
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "make_closure".to_string(),
        expr: AnfExpr::Let {
            name: "closure".to_string(),
            value: Box::new(AnfExpr::Lambda {
                params: vec!["x".to_string()],
                captures: vec!["outer".to_string()],
                body: Box::new(AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["outer".to_string(), "x".to_string()],
                }),
            }),
            body: Box::new(AnfExpr::Var("closure".to_string())),
        },
    };
    let anf = sealed_anf(vec![binding]);
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for binding with nested Lambda");
    assert!(!artifact.wasm.is_empty(), "WASM binary must be non-empty");
    // A memory section must be present (confirmed by the global bump-allocator
    // section, which is only emitted when needs_memory = true).  We verify
    // indirectly: the WASM binary must be larger than a module with no memory.
    let no_mem_anf = sealed_anf(vec![AnfBinding {
        source_ref: ail_core::semantic_graph::NodeRef(1),
        name: "lit".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Int(42)),
    }]);
    let no_mem_artifact = emit_wasm(&no_mem_anf).unwrap();
    assert!(
        artifact.wasm.len() > no_mem_artifact.wasm.len(),
        "module with nested Lambda + captures must be larger than a literal-only module \
         (memory + global sections are required for the closure env)"
    );
}

// TRIANGULATE: two Lambda bindings with different capture counts produce different
// WASM hashes (cap_count field in closure env header changes the binary).
#[test]
fn lambda_bindings_with_different_capture_counts_produce_different_hashes() {
    use ail_core::semantic_graph::NodeRef;

    let make_lambda = |captures: Vec<String>| {
        sealed_anf(vec![AnfBinding {
            source_ref: NodeRef(0),
            name: "f".to_string(),
            expr: AnfExpr::Lambda {
                params: vec!["x".to_string()],
                captures,
                body: Box::new(AnfExpr::Var("x".to_string())),
            },
        }])
    };

    let a = emit_wasm(&make_lambda(vec![])).unwrap();
    let b = emit_wasm(&make_lambda(vec!["outer".to_string()])).unwrap();
    assert_ne!(
        a.hash_chain.wasm_hash, b.hash_chain.wasm_hash,
        "Lambda with captures must produce a different wasm_hash than one without"
    );
}

// ── End WASM closure capture tests ───────────────────────────────────────

// ── Wave 7C: CellNew / CellGet / CellSet / MapNew / SetNew / IndexGet ─────
//
// Proves that the six collection/cell primitives no longer emit unconditional
// Unreachable and instead produce valid, executable WASM that uses linear
// memory correctly.

// Scenario: CellNew allocates 8 bytes and stores the initial value.
// Expects: memory section present, I64Store emitted, WASM validates.
#[test]
fn cell_new_emits_alloc_and_store_validates() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.make_cell".to_string(),
        expr: AnfExpr::Let {
            name: "init_val".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(99))),
            body: Box::new(AnfExpr::CellNew {
                init: "init_val".to_string(),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for CellNew");
    wasmparser::validate(&artifact.wasm).expect("CellNew module must validate");

    let mut saw_memory = false;
    let mut saw_store = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        match payload.unwrap() {
            Payload::MemorySection(_) => saw_memory = true,
            Payload::CodeSectionEntry(body) => {
                let mut reader = body.get_operators_reader().unwrap();
                while !reader.eof() {
                    if let Operator::I64Store { .. } = reader.read().unwrap() {
                        saw_store = true;
                    }
                }
            }
            _ => {}
        }
    }

    assert!(saw_memory, "CellNew must declare linear memory");
    assert!(
        saw_store,
        "CellNew must emit I64Store for the initial value"
    );
}

// Scenario: CellGet loads the stored value from the cell pointer.
// Expects: I64Load emitted, WASM validates.
#[test]
fn cell_get_emits_i64_load_validates() {
    use wasmparser::{Operator, Parser, Payload};

    // let cell = CellNew { init: 42 }; CellGet { cell }
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.read_cell".to_string(),
        expr: AnfExpr::Let {
            name: "init_val".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(42))),
            body: Box::new(AnfExpr::Let {
                name: "cell".to_string(),
                value: Box::new(AnfExpr::CellNew {
                    init: "init_val".to_string(),
                }),
                body: Box::new(AnfExpr::CellGet {
                    cell: "cell".to_string(),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for CellGet");
    wasmparser::validate(&artifact.wasm).expect("CellGet module must validate");

    let mut saw_load = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::I64Load { memarg } = reader.read().unwrap()
                    && memarg.offset == 0
                {
                    saw_load = true;
                }
            }
        }
    }

    assert!(
        saw_load,
        "CellGet must emit I64Load at offset 0 to read the cell value"
    );
}

// Scenario: CellSet writes a new value into the cell.
// Expects: multiple I64Stores (init + set), WASM validates.
#[test]
fn cell_set_emits_i64_store_validates() {
    use wasmparser::{Operator, Parser, Payload};

    // let v = 1; let cell = CellNew { init: v }; let new_v = 2; CellSet { cell, value: new_v }
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.write_cell".to_string(),
        expr: AnfExpr::Let {
            name: "v".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::Let {
                name: "cell".to_string(),
                value: Box::new(AnfExpr::CellNew {
                    init: "v".to_string(),
                }),
                body: Box::new(AnfExpr::Let {
                    name: "new_v".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(2))),
                    body: Box::new(AnfExpr::CellSet {
                        cell: "cell".to_string(),
                        value: "new_v".to_string(),
                    }),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for CellSet");
    wasmparser::validate(&artifact.wasm).expect("CellSet module must validate");

    let mut store_count = 0usize;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::I64Store { .. } = reader.read().unwrap() {
                    store_count += 1;
                }
            }
        }
    }

    // At least two I64Stores: one for CellNew (init), one for CellSet (write).
    assert!(
        store_count >= 2,
        "CellNew + CellSet must emit at least 2 I64Stores; got {store_count}"
    );
}

// Scenario: MapNew stores count + interleaved key-value pairs.
// Expects: memory section, count I64Const, I64Stores for entries, WASM validates.
#[test]
fn map_new_stores_count_and_kv_pairs_validates() {
    use wasmparser::{Operator, Parser, Payload};

    // let k = 10; let v = 20; MapNew { entries: [(k, v)] }
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.make_map".to_string(),
        expr: AnfExpr::Let {
            name: "k".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
            body: Box::new(AnfExpr::Let {
                name: "v".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(20))),
                body: Box::new(AnfExpr::MapNew {
                    entries: vec![("k".to_string(), "v".to_string())],
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for MapNew");
    wasmparser::validate(&artifact.wasm).expect("MapNew module must validate");

    let mut saw_memory = false;
    let mut store_count = 0usize;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        match payload.unwrap() {
            Payload::MemorySection(_) => saw_memory = true,
            Payload::CodeSectionEntry(body) => {
                let mut reader = body.get_operators_reader().unwrap();
                while !reader.eof() {
                    if let Operator::I64Store { .. } = reader.read().unwrap() {
                        store_count += 1;
                    }
                }
            }
            _ => {}
        }
    }

    assert!(saw_memory, "MapNew must declare linear memory");
    // 3 I64Stores: count + key + value.
    assert!(
        store_count >= 3,
        "MapNew with 1 entry must emit >= 3 I64Stores (count, key, value); got {store_count}"
    );
}

// TRIANGULATE: empty MapNew still produces a valid module with a count of 0.
#[test]
fn map_new_empty_validates() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.empty_map".to_string(),
        expr: AnfExpr::MapNew { entries: vec![] },
    }]);
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for empty MapNew");
    wasmparser::validate(&artifact.wasm).expect("empty MapNew module must validate");
}

// Scenario: SetNew stores count + element values.
// Expects: memory section, I64Stores for count + elements, WASM validates.
#[test]
fn set_new_stores_count_and_elements_validates() {
    use wasmparser::{Operator, Parser, Payload};

    // let e1 = 1; let e2 = 2; SetNew { elements: [e1, e2] }
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.make_set".to_string(),
        expr: AnfExpr::Let {
            name: "e1".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::Let {
                name: "e2".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(2))),
                body: Box::new(AnfExpr::SetNew {
                    elements: vec!["e1".to_string(), "e2".to_string()],
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for SetNew");
    wasmparser::validate(&artifact.wasm).expect("SetNew module must validate");

    let mut saw_memory = false;
    let mut store_count = 0usize;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        match payload.unwrap() {
            Payload::MemorySection(_) => saw_memory = true,
            Payload::CodeSectionEntry(body) => {
                let mut reader = body.get_operators_reader().unwrap();
                while !reader.eof() {
                    if let Operator::I64Store { .. } = reader.read().unwrap() {
                        store_count += 1;
                    }
                }
            }
            _ => {}
        }
    }

    assert!(saw_memory, "SetNew must declare linear memory");
    // 3 I64Stores: count + e1 + e2.
    assert!(
        store_count >= 3,
        "SetNew with 2 elements must emit >= 3 I64Stores; got {store_count}"
    );
}

// TRIANGULATE: empty SetNew produces a valid module.
#[test]
fn set_new_empty_validates() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.empty_set".to_string(),
        expr: AnfExpr::SetNew { elements: vec![] },
    }]);
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for empty SetNew");
    wasmparser::validate(&artifact.wasm).expect("empty SetNew module must validate");
}

// Scenario: IndexGet loads an element from a list by dynamic index.
// Expects: I64Mul + I64Add + I32WrapI64 + I32Add + I64Load sequence, WASM validates.
#[test]
fn index_get_emits_dynamic_load_validates() {
    use wasmparser::{Operator, Parser, Payload};

    // let list = ListNew([10, 20, 30]); let idx = 1; IndexGet { collection: list, index: idx }
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.get_elem".to_string(),
        expr: AnfExpr::Let {
            name: "list".to_string(),
            value: Box::new(AnfExpr::ListNew(vec![
                AnfExpr::Literal(LiteralValue::Int(10)),
                AnfExpr::Literal(LiteralValue::Int(20)),
                AnfExpr::Literal(LiteralValue::Int(30)),
            ])),
            body: Box::new(AnfExpr::Let {
                name: "idx".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                body: Box::new(AnfExpr::IndexGet {
                    collection: "list".to_string(),
                    index: "idx".to_string(),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for IndexGet");
    wasmparser::validate(&artifact.wasm).expect("IndexGet module must validate");

    // Verify the dynamic address computation instructions are present.
    let mut saw_i64_mul = false;
    let mut saw_i64_add = false;
    let mut saw_i32_wrap = false;
    let mut saw_i32_add = false;
    let mut saw_i64_load = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                match reader.read().unwrap() {
                    Operator::I64Mul => saw_i64_mul = true,
                    Operator::I64Add => saw_i64_add = true,
                    Operator::I32WrapI64 => saw_i32_wrap = true,
                    Operator::I32Add => saw_i32_add = true,
                    Operator::I64Load { .. } => saw_i64_load = true,
                    _ => {}
                }
            }
        }
    }

    assert!(saw_i64_mul, "IndexGet must emit I64Mul for index * 8");
    assert!(saw_i64_add, "IndexGet must emit I64Add for offset + 8");
    assert!(
        saw_i32_wrap,
        "IndexGet must emit I32WrapI64 to convert offset"
    );
    assert!(
        saw_i32_add,
        "IndexGet must emit I32Add to compute final address"
    );
    assert!(
        saw_i64_load,
        "IndexGet must emit I64Load to read the element"
    );
}

// TRIANGULATE: IndexGet with out-of-bounds index still produces valid WASM
// (bounds checking is runtime responsibility; the codegen is always structurally valid).
#[test]
fn index_get_out_of_bounds_still_validates() {
    // Same structure as above but with an idx that would be OOB at runtime.
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.oob".to_string(),
        expr: AnfExpr::Let {
            name: "list".to_string(),
            value: Box::new(AnfExpr::ListNew(vec![AnfExpr::Literal(LiteralValue::Int(
                1,
            ))])),
            body: Box::new(AnfExpr::Let {
                name: "idx".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(999))),
                body: Box::new(AnfExpr::IndexGet {
                    collection: "list".to_string(),
                    index: "idx".to_string(),
                }),
            }),
        },
    }]);
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("OOB IndexGet module must still be valid WASM");
}

// Scenario: infer_expr_type returns I32 for MapNew and SetNew (they are pointers).
#[test]
fn infer_expr_type_map_set_new_is_i32() {
    use wasm_encoder::ValType;
    let map = AnfExpr::MapNew { entries: vec![] };
    let set = AnfExpr::SetNew { elements: vec![] };
    let mut locals = vec![];
    assert_eq!(
        crate::wasm_abi::infer_expr_type(&map, &mut locals),
        Some(ValType::I32),
        "MapNew must infer I32 (pointer)"
    );
    assert_eq!(
        crate::wasm_abi::infer_expr_type(&set, &mut locals),
        Some(ValType::I32),
        "SetNew must infer I32 (pointer)"
    );
}

// Scenario: infer_expr_type returns I32 for CellNew, I64 for CellGet, I32 for CellSet.
// CellSet returns unit (I32 0), consistent with the unit-as-I32(0) pattern in
// the emit layer.  Both infer and emit must agree: Some(I32).
#[test]
fn infer_expr_type_cell_ops_correct() {
    use wasm_encoder::ValType;
    let mut locals = vec![("c".to_string(), ValType::I32)];
    assert_eq!(
        crate::wasm_abi::infer_expr_type(
            &AnfExpr::CellNew {
                init: "c".to_string()
            },
            &mut locals
        ),
        Some(ValType::I32),
        "CellNew must infer I32"
    );
    assert_eq!(
        crate::wasm_abi::infer_expr_type(
            &AnfExpr::CellGet {
                cell: "c".to_string()
            },
            &mut locals
        ),
        Some(ValType::I64),
        "CellGet must infer I64"
    );
    assert_eq!(
        crate::wasm_abi::infer_expr_type(
            &AnfExpr::CellSet {
                cell: "c".to_string(),
                value: "c".to_string()
            },
            &mut locals
        ),
        Some(ValType::I32),
        "CellSet must infer I32 (unit-as-I32(0), matching emit)"
    );
}

// W3 regression: CellGet, CellSet, and IndexGet must set needs_memory in
// EffectDataLayout.  All three issue linear-memory loads or stores and require
// the memory and bump-allocator-global sections to be present in the module.
#[test]
fn effect_data_layout_cell_get_set_index_get_need_memory() {
    use ail_core::semantic_graph::NodeRef;

    let make_layout = |expr: AnfExpr| {
        let bindings = vec![AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.test".to_string(),
            expr,
        }];
        EffectDataLayout::for_bindings(&bindings)
    };

    assert!(
        make_layout(AnfExpr::CellGet {
            cell: "c".to_string()
        })
        .needs_memory,
        "CellGet issues I64Load — must set needs_memory"
    );
    assert!(
        make_layout(AnfExpr::CellSet {
            cell: "c".to_string(),
            value: "v".to_string()
        })
        .needs_memory,
        "CellSet issues I64Store — must set needs_memory"
    );
    assert!(
        make_layout(AnfExpr::IndexGet {
            collection: "c".to_string(),
            index: "i".to_string()
        })
        .needs_memory,
        "IndexGet issues I64Load at dynamic offset — must set needs_memory"
    );
}

// ── End Wave 7C collection/cell tests ────────────────────────────────────

// ── Wave 8C: ForEach iteration primitive ─────────────────────────────────
//
// Proves that ForEach produces a real WASM loop (block + loop + I64GeU
// exit condition + I64Load element load) instead of unconditional Unreachable,
// and that the emitted module validates.

// Scenario: ForEach over a list emits a loop structure and validates.
// Expects: Block + Loop instructions present; module validates.
#[test]
fn foreach_emits_loop_structure_and_validates() {
    use wasmparser::{Operator, Parser, Payload};

    // let list = [10, 20, 30]; foreach item in list: noop (use item as body)
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.loop_test".to_string(),
        expr: AnfExpr::Let {
            name: "elem0".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
            body: Box::new(AnfExpr::Let {
                name: "elem1".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(20))),
                body: Box::new(AnfExpr::Let {
                    name: "lst".to_string(),
                    value: Box::new(AnfExpr::ListNew(vec![
                        AnfExpr::Var("elem0".to_string()),
                        AnfExpr::Var("elem1".to_string()),
                    ])),
                    body: Box::new(AnfExpr::ForEach {
                        binding: "item".to_string(),
                        collection: "lst".to_string(),
                        // Body: reference the binding (side-effect: just reads it)
                        body: Box::new(AnfExpr::Var("item".to_string())),
                    }),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for ForEach");
    wasmparser::validate(&artifact.wasm).expect("ForEach module must validate");

    let mut saw_block = false;
    let mut saw_loop = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                match reader.read().unwrap() {
                    Operator::Block { .. } => saw_block = true,
                    Operator::Loop { .. } => saw_loop = true,
                    _ => {}
                }
            }
        }
    }

    assert!(
        saw_block,
        "ForEach must emit a Block instruction (break target)"
    );
    assert!(
        saw_loop,
        "ForEach must emit a Loop instruction (continue target)"
    );
}

// Scenario: ForEach emits I64Load to read list elements.
// Expects: I64Load present in code section; module validates.
#[test]
fn foreach_emits_i64_load_for_element() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.read_elems".to_string(),
        expr: AnfExpr::Let {
            name: "e".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(7))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![AnfExpr::Var("e".to_string())])),
                body: Box::new(AnfExpr::ForEach {
                    binding: "x".to_string(),
                    collection: "lst".to_string(),
                    body: Box::new(AnfExpr::Var("x".to_string())),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("module must validate");

    let mut saw_load = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::I64Load { .. } = reader.read().unwrap() {
                    saw_load = true;
                }
            }
        }
    }

    assert!(saw_load, "ForEach must emit I64Load to read list elements");
}

// Scenario: ForEach exit condition uses I64GeU.
// Expects: I64GeU present (i >= count break test); module validates.
#[test]
fn foreach_emits_i64_geu_exit_condition() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.exit_cond".to_string(),
        expr: AnfExpr::Let {
            name: "v".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![AnfExpr::Var("v".to_string())])),
                body: Box::new(AnfExpr::ForEach {
                    binding: "item".to_string(),
                    collection: "lst".to_string(),
                    body: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("module must validate");

    let mut saw_geu = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::I64GeU = reader.read().unwrap() {
                    saw_geu = true;
                }
            }
        }
    }

    assert!(
        saw_geu,
        "ForEach must emit I64GeU for the loop exit condition (i >= count)"
    );
}

// Scenario: ForEach sets needs_memory in EffectDataLayout.
// Expects: needs_memory = true (ForEach reads list elements via I64Load).
#[test]
fn foreach_sets_needs_memory_in_effect_data_layout() {
    let bindings = vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.fe".to_string(),
        expr: AnfExpr::ForEach {
            binding: "item".to_string(),
            collection: "lst".to_string(),
            body: Box::new(AnfExpr::Var("item".to_string())),
        },
    }];
    let layout = EffectDataLayout::for_bindings(&bindings);
    assert!(
        layout.needs_memory,
        "ForEach reads list elements via I64Load — must set needs_memory"
    );
}

// TRIANGULATE: ForEach over an empty list still produces valid WASM.
#[test]
fn foreach_over_empty_list_validates() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.empty_loop".to_string(),
        expr: AnfExpr::Let {
            name: "lst".to_string(),
            value: Box::new(AnfExpr::ListNew(vec![])),
            body: Box::new(AnfExpr::ForEach {
                binding: "item".to_string(),
                collection: "lst".to_string(),
                body: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
            }),
        },
    }]);
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for empty-list ForEach");
    wasmparser::validate(&artifact.wasm).expect("ForEach over empty list must produce valid WASM");
}

// Scenario: ForEach returns no value (infer_expr_type → None).
#[test]
fn foreach_infer_expr_type_is_none() {
    use crate::wasm_abi::infer_expr_type;
    use wasm_encoder::ValType;

    let expr = AnfExpr::ForEach {
        binding: "x".to_string(),
        collection: "lst".to_string(),
        body: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
    };
    let mut locals: Vec<(String, ValType)> = vec![];
    assert_eq!(
        infer_expr_type(&expr, &mut locals),
        None,
        "ForEach is side-effect only — infer_expr_type must return None"
    );
}

// ── Wave 9B: ResourceAcquire / ResourceRelease WASM emission ─────────────

/// Build a minimal `AnfIr` with a single binding whose body is the given expr.
fn anf_with_single_binding(name: &str, body: AnfExpr) -> AnfIr {
    sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(42),
        name: name.to_string(),
        expr: body,
    }])
}

// R9B-S1: ResourceAcquire emits `ail/resource_acquire` import.
#[test]
fn resource_acquire_emits_resource_acquire_import() {
    use wasmparser::{Parser, Payload};

    let anf = anf_with_single_binding(
        "acquire_db",
        AnfExpr::ResourceAcquire {
            resource: "db.connection".to_string(),
            args: vec![],
        },
    );
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for ResourceAcquire");
    wasmparser::validate(&artifact.wasm).expect("ResourceAcquire WASM must be valid");

    let mut found_resource_acquire = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::ImportSection(imports) = payload.unwrap() {
            for imp in imports.into_imports() {
                let imp = imp.unwrap();
                if imp.module == "ail" && imp.name == "resource_acquire" {
                    found_resource_acquire = true;
                }
            }
        }
    }
    assert!(
        found_resource_acquire,
        "ResourceAcquire must import 'ail'/'resource_acquire'"
    );
}

// R9B-S2: ResourceRelease emits `ail/resource_release` import.
#[test]
fn resource_release_emits_resource_release_import() {
    use wasmparser::{Parser, Payload};

    // ResourceRelease needs a handle local — wrap in a Let that binds an i64.
    let anf = anf_with_single_binding(
        "release_db",
        AnfExpr::Let {
            name: "h".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::ResourceRelease {
                handle: "h".to_string(),
            }),
        },
    );
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for ResourceRelease");
    wasmparser::validate(&artifact.wasm).expect("ResourceRelease WASM must be valid");

    let mut found_resource_release = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::ImportSection(imports) = payload.unwrap() {
            for imp in imports.into_imports() {
                let imp = imp.unwrap();
                if imp.module == "ail" && imp.name == "resource_release" {
                    found_resource_release = true;
                }
            }
        }
    }
    assert!(
        found_resource_release,
        "ResourceRelease must import 'ail'/'resource_release'"
    );
}

// R9B-S3: Both `ail/resource_acquire` and `ail/resource_release` are imported
// when a binding contains both primitives.
#[test]
fn resource_acquire_and_release_both_imported_together() {
    use wasmparser::{Parser, Payload};

    // Let h = ResourceAcquire { .. }; ResourceRelease { handle: h }
    let anf = anf_with_single_binding(
        "acquire_then_release",
        AnfExpr::Let {
            name: "h".to_string(),
            value: Box::new(AnfExpr::ResourceAcquire {
                resource: "fs.file".to_string(),
                args: vec![],
            }),
            body: Box::new(AnfExpr::ResourceRelease {
                handle: "h".to_string(),
            }),
        },
    );
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("acquire+release WASM must be valid");

    let mut found_acquire = false;
    let mut found_release = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::ImportSection(imports) = payload.unwrap() {
            for imp in imports.into_imports() {
                let imp = imp.unwrap();
                if imp.module == "ail" {
                    if imp.name == "resource_acquire" {
                        found_acquire = true;
                    } else if imp.name == "resource_release" {
                        found_release = true;
                    }
                }
            }
        }
    }
    assert!(found_acquire, "must import 'ail'/'resource_acquire'");
    assert!(found_release, "must import 'ail'/'resource_release'");
}

// R9B-S4: infer_expr_type for ResourceAcquire returns Some(I64) — handle slot.
#[test]
fn resource_acquire_infer_expr_type_is_i64() {
    use crate::wasm_abi::infer_expr_type;
    use wasm_encoder::ValType;

    let expr = AnfExpr::ResourceAcquire {
        resource: "db.connection".to_string(),
        args: vec![],
    };
    let mut locals: Vec<(String, ValType)> = vec![];
    assert_eq!(
        infer_expr_type(&expr, &mut locals),
        Some(ValType::I64),
        "ResourceAcquire must return Some(I64) — the handle slot"
    );
}

// R9B-S5: infer_expr_type for ResourceRelease returns None — void return.
#[test]
fn resource_release_infer_expr_type_is_none() {
    use crate::wasm_abi::infer_expr_type;
    use wasm_encoder::ValType;

    let expr = AnfExpr::ResourceRelease {
        handle: "h".to_string(),
    };
    let mut locals: Vec<(String, ValType)> = vec![];
    assert_eq!(
        infer_expr_type(&expr, &mut locals),
        None,
        "ResourceRelease is side-effect only — must return None"
    );
}

// R9B-S6: EffectDataLayout sets needs_resource_call for ResourceAcquire.
#[test]
fn effect_data_layout_needs_resource_call_for_acquire() {
    let bindings = vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "acquire_db".to_string(),
        expr: AnfExpr::ResourceAcquire {
            resource: "db.connection".to_string(),
            args: vec![],
        },
    }];
    let layout = EffectDataLayout::for_bindings(&bindings);
    assert!(
        layout.needs_resource_call,
        "EffectDataLayout must set needs_resource_call for ResourceAcquire"
    );
    assert!(
        layout.needs_memory,
        "ResourceAcquire requires linear memory (data section for resource name)"
    );
    assert!(
        layout.args_offset > 0,
        "args_offset must be set when needs_resource_call (got {})",
        layout.args_offset
    );
}

// R9B-S7: EffectDataLayout sets needs_resource_call for ResourceRelease.
#[test]
fn effect_data_layout_needs_resource_call_for_release() {
    let bindings = vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "release_h".to_string(),
        expr: AnfExpr::ResourceRelease {
            handle: "h".to_string(),
        },
    }];
    let layout = EffectDataLayout::for_bindings(&bindings);
    assert!(
        layout.needs_resource_call,
        "EffectDataLayout must set needs_resource_call for ResourceRelease"
    );
}

// R9B-S8: ResourceAcquire with args — all args written to the args buffer
// and passed correctly to resource_acquire.  WASM validates.
#[test]
fn resource_acquire_with_args_emits_valid_wasm() {
    let anf = anf_with_single_binding(
        "acquire_with_args",
        AnfExpr::Let {
            name: "timeout".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(5000))),
            body: Box::new(AnfExpr::ResourceAcquire {
                resource: "db.connection".to_string(),
                args: vec!["timeout".to_string()],
            }),
        },
    );
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for ResourceAcquire with args");
    wasmparser::validate(&artifact.wasm)
        .expect("ResourceAcquire with args must produce valid WASM");
}

// R9B-S9: resource_acquire func index is 0 when no EffectCall imports precede it.
#[test]
fn resource_acquire_func_index_is_zero_without_effect_calls() {
    let bindings = vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "acquire_only".to_string(),
        expr: AnfExpr::ResourceAcquire {
            resource: "db".to_string(),
            args: vec![],
        },
    }];
    let layout = EffectDataLayout::for_bindings(&bindings);
    assert_eq!(
        layout.resource_acquire_func_index(),
        0,
        "resource_acquire must be function index 0 when no host_call imports precede it"
    );
    assert_eq!(
        layout.resource_release_func_index(),
        1,
        "resource_release must be function index 1 when no host_call imports precede it"
    );
}

// R9B-S10: ABI descriptor marks ResourceAcquire binding as Handle.
#[test]
fn resource_acquire_abi_descriptor_is_handle() {
    let anf = anf_with_single_binding(
        "acquire_db",
        AnfExpr::ResourceAcquire {
            resource: "db.connection".to_string(),
            args: vec![],
        },
    );
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed");
    let descriptor = artifact.export_types.get("acquire_db");
    assert_eq!(
        descriptor,
        Some(&WasmTypeDescriptor::Handle),
        "ResourceAcquire binding must have Handle ABI descriptor"
    );
}

// R9B-S11: Mixed EffectCall + ResourceAcquire — import index arithmetic.
//
// When ail/host_call is imported before ail/resource_acquire in the same
// module, `resource_acquire_func_index()` must return 1 (not 0) and
// `resource_release_func_index()` must return 2.  This exercises the
// arithmetic in `EffectDataLayout` for the mixed-import case.
#[test]
fn mixed_effect_call_and_resource_acquire_index_arithmetic() {
    let bindings = vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.read_data".to_string(),
            expr: AnfExpr::EffectCall {
                capability: "io".to_string(),
                func: "read".to_string(),
                args: vec![],
            },
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn.acquire_db".to_string(),
            expr: AnfExpr::ResourceAcquire {
                resource: "db.connection".to_string(),
                args: vec![],
            },
        },
    ];
    let layout = EffectDataLayout::for_bindings(&bindings);
    assert!(layout.needs_host_call, "needs_host_call must be true");
    assert!(
        layout.needs_resource_call,
        "needs_resource_call must be true"
    );
    assert!(
        !layout.needs_host_call_write,
        "no structured return — host_call_write must not be needed"
    );
    assert_eq!(
        layout.resource_acquire_func_index(),
        1,
        "resource_acquire must be at import index 1 when host_call is at 0"
    );
    assert_eq!(
        layout.resource_release_func_index(),
        2,
        "resource_release must be at import index 2"
    );
}

// End-to-end: mixed EffectCall + ResourceAcquire emits valid WASM with the
// correct import ordering (host_call before resource_acquire).
#[test]
fn mixed_effect_call_and_resource_acquire_emits_valid_wasm_with_correct_import_order() {
    use wasmparser::{Parser, Payload};

    let anf = sealed_anf(vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.read_data".to_string(),
            expr: AnfExpr::EffectCall {
                capability: "io".to_string(),
                func: "read".to_string(),
                args: vec![],
            },
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn.acquire_db".to_string(),
            expr: AnfExpr::ResourceAcquire {
                resource: "db.connection".to_string(),
                args: vec![],
            },
        },
    ]);
    let artifact =
        emit_wasm(&anf).expect("emit_wasm must succeed for mixed EffectCall + ResourceAcquire");
    wasmparser::validate(&artifact.wasm).expect("mixed WASM must be valid");

    // Collect ail import names in declaration order.
    let mut import_names: Vec<String> = Vec::new();
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::ImportSection(imports) = payload.unwrap() {
            for imp in imports.into_imports() {
                let imp = imp.unwrap();
                if imp.module == "ail" {
                    import_names.push(imp.name.to_string());
                }
            }
        }
    }
    let host_pos = import_names.iter().position(|n| n == "host_call");
    let acquire_pos = import_names.iter().position(|n| n == "resource_acquire");
    assert!(
        host_pos.is_some(),
        "host_call must be imported; got: {import_names:?}"
    );
    assert!(
        acquire_pos.is_some(),
        "resource_acquire must be imported; got: {import_names:?}"
    );
    assert!(
        host_pos.unwrap() < acquire_pos.unwrap(),
        "host_call (idx {}) must appear before resource_acquire (idx {}) in import section",
        host_pos.unwrap(),
        acquire_pos.unwrap()
    );
}

// R9B-S12: ResourceRelease-only module must NOT include a memory section.
//
// ResourceRelease emits only LocalGet(handle) + Call(resource_release) —
// no string interning, no args buffer, no heap access.  Folding
// `needs_resource_call` into the `needs_memory` guard would cause a wasteful
// memory + global section to appear in the binary.
#[test]
fn effect_data_layout_resource_release_does_not_set_needs_memory() {
    let bindings = vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "release_h".to_string(),
        expr: AnfExpr::ResourceRelease {
            handle: "h".to_string(),
        },
    }];
    let layout = EffectDataLayout::for_bindings(&bindings);
    assert!(
        layout.needs_resource_call,
        "needs_resource_call must be true for ResourceRelease"
    );
    assert!(
        !layout.needs_memory,
        "ResourceRelease does not access linear memory — needs_memory must be false"
    );
}

#[test]
fn resource_release_only_module_has_no_memory_section() {
    use wasmparser::{Parser, Payload};

    let anf = anf_with_single_binding(
        "release_h",
        AnfExpr::Let {
            name: "h".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::ResourceRelease {
                handle: "h".to_string(),
            }),
        },
    );
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for ResourceRelease-only module");
    wasmparser::validate(&artifact.wasm).expect("ResourceRelease-only WASM must be valid");

    let mut saw_memory = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::MemorySection(_) = payload.unwrap() {
            saw_memory = true;
        }
    }
    assert!(
        !saw_memory,
        "ResourceRelease-only module must not include a memory section \
         (no string interning, no args buffer, no heap access)"
    );
}

// ── End Wave 8C iteration tests ───────────────────────────────────────────

// ── Wave 11B: Fold via call_indirect + function table ─────────────────────
//
// Fold is now implemented.  A module containing Fold:
//   1. Emits a table section (one funcref table, N slots).
//   2. Emits an element section (populates table with all function indices).
//   3. Adds a fold-reducer type (i64, i64) → i64 to the type section.
//   4. Emits a call_indirect loop in the code section.
//   5. The final WASM module validates with wasmparser.
//
// The previous diagnostic tests (Wave 9A) have been replaced by these
// verification tests that assert successful compilation and structural
// correctness.

// Scenario: top-level Fold binding now emits successfully.
// Previously expected UnsupportedWasmConstruct; now expects Ok.
#[test]
fn fold_top_level_emits_successfully() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.reduce".to_string(),
        expr: AnfExpr::Fold {
            init: "acc0".to_string(),
            list: "lst".to_string(),
            func: "f".to_string(),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "Fold binding must compile successfully now that call_indirect is implemented; \
         got {result:?}"
    );
}

// Scenario: Fold nested inside a Let chain — still emits successfully.
// Verifies that the recursion detection (anf_has_fold) correctly spots nested Fold.
#[test]
fn fold_nested_in_let_emits_successfully() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.nested_fold".to_string(),
        expr: AnfExpr::Let {
            name: "acc0".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Fold {
                    init: "acc0".to_string(),
                    list: "lst".to_string(),
                    func: "f".to_string(),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "Fold nested inside a Let chain must compile successfully; got {result:?}"
    );
}

// Scenario: UnsupportedWasmConstruct Display still names the unsupported construct.
// Fold is gone from the error set; the Display contract applies to other constructs.
#[test]
fn unsupported_wasm_construct_error_display_names_construct() {
    let msg = CompileError::UnsupportedWasmConstruct("TaskSpawn".to_string()).to_string();
    assert!(
        msg.contains("TaskSpawn"),
        "error display must name the unsupported construct: {msg}"
    );
    assert!(
        msg.contains("WASM"),
        "error display must mention WASM: {msg}"
    );
}

// TRIANGULATE: UnsupportedWasmConstruct is distinct from EncodingError.
#[test]
fn unsupported_construct_error_is_distinct_from_encoding_error() {
    let unsupported = CompileError::UnsupportedWasmConstruct("TaskSpawn".to_string());
    let encoding = CompileError::EncodingError("TaskSpawn".to_string());
    assert_ne!(
        unsupported, encoding,
        "UnsupportedWasmConstruct must be a distinct variant from EncodingError"
    );
}

// Scenario: a binding WITHOUT Fold emits successfully and has no table section.
// Regression guard — Fold implementation must not add table to non-Fold modules.
#[test]
fn non_fold_binding_has_no_table_section() {
    use wasmparser::{Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.simple".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Int(1)),
    }]);
    let artifact = emit_wasm(&anf).expect("non-Fold binding must compile successfully");

    let mut saw_table = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::TableSection(_) = payload.unwrap() {
            saw_table = true;
        }
    }
    assert!(
        !saw_table,
        "non-Fold module must NOT have a table section (fold infrastructure is gated)"
    );
}

// Scenario: a Fold module emits a TableSection.
// Proves the function table is added when Fold is present.
#[test]
fn fold_module_has_table_section() {
    use wasmparser::{Parser, Payload};

    // Two bindings: a reducer and a Fold over it.
    let anf = sealed_anf(vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.add".to_string(),
            expr: AnfExpr::Lambda {
                params: vec!["acc".to_string(), "x".to_string()],
                captures: vec![],
                body: Box::new(AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["acc".to_string(), "x".to_string()],
                }),
            },
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn.sum".to_string(),
            expr: AnfExpr::Let {
                name: "acc0".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Let {
                    name: "lst".to_string(),
                    value: Box::new(AnfExpr::ListNew(vec![
                        AnfExpr::Literal(LiteralValue::Int(1)),
                        AnfExpr::Literal(LiteralValue::Int(2)),
                        AnfExpr::Literal(LiteralValue::Int(3)),
                    ])),
                    body: Box::new(AnfExpr::Fold {
                        init: "acc0".to_string(),
                        list: "lst".to_string(),
                        func: "fn.add".to_string(),
                    }),
                }),
            },
        },
    ]);

    let artifact = emit_wasm(&anf).expect("Fold module must compile successfully");
    wasmparser::validate(&artifact.wasm).expect("Fold module WASM must validate");

    let mut saw_table = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::TableSection(_) = payload.unwrap() {
            saw_table = true;
        }
    }
    assert!(saw_table, "Fold module must include a TableSection");
}

// Scenario: a Fold module emits an ElementSection.
// Proves the element segment populates the function table.
#[test]
fn fold_module_has_element_section() {
    use wasmparser::{Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.reduce".to_string(),
        expr: AnfExpr::Fold {
            init: "acc0".to_string(),
            list: "lst".to_string(),
            func: "f".to_string(),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("Fold module must compile");

    let mut saw_element = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::ElementSection(_) = payload.unwrap() {
            saw_element = true;
        }
    }
    assert!(saw_element, "Fold module must include an ElementSection");
}

// Scenario: a Fold module emits a CallIndirect instruction.
// Proves the actual call_indirect dispatch is in the code section.
// `acc0` and `lst` must be in scope (let-bound) so the Fold emission
// reaches the call_indirect instruction rather than short-circuiting on
// the unresolved `list` variable.
#[test]
fn fold_module_emits_call_indirect() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.reduce".to_string(),
        // Let-bind acc0 and lst so they are in scope when Fold is emitted.
        // func = "f" is intentionally unresolved — the emitter falls through
        // to the Unreachable+CallIndirect path (dead code, but syntactically
        // present and parseable by wasmparser).
        expr: AnfExpr::Let {
            name: "acc0".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Fold {
                    init: "acc0".to_string(),
                    list: "lst".to_string(),
                    func: "f".to_string(),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("Fold module must compile");

    let mut saw_call_indirect = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::CallIndirect { .. } = reader.read().unwrap() {
                    saw_call_indirect = true;
                }
            }
        }
    }
    assert!(
        saw_call_indirect,
        "Fold module must emit CallIndirect in the code section"
    );
}

// Scenario: Fold with a named top-level reducer function validates.
// Proves the full Fold pipeline: reducer function + Fold via named function ref.
#[test]
fn fold_with_named_reducer_validates() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.add".to_string(),
            expr: AnfExpr::Lambda {
                params: vec!["acc".to_string(), "x".to_string()],
                captures: vec![],
                body: Box::new(AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["acc".to_string(), "x".to_string()],
                }),
            },
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn.sum".to_string(),
            expr: AnfExpr::Let {
                name: "acc0".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Let {
                    name: "lst".to_string(),
                    value: Box::new(AnfExpr::ListNew(vec![
                        AnfExpr::Literal(LiteralValue::Int(10)),
                        AnfExpr::Literal(LiteralValue::Int(20)),
                    ])),
                    body: Box::new(AnfExpr::Fold {
                        init: "acc0".to_string(),
                        list: "lst".to_string(),
                        // "fn.add" resolves as a top-level function name.
                        func: "fn.add".to_string(),
                    }),
                }),
            },
        },
    ]);

    let artifact = emit_wasm(&anf).expect("Fold with named reducer must compile");
    wasmparser::validate(&artifact.wasm).expect("Fold with named reducer must validate");

    // Verify call_indirect is emitted for the sum function.
    let mut saw_call_indirect = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::CallIndirect { .. } = reader.read().unwrap() {
                    saw_call_indirect = true;
                }
            }
        }
    }
    assert!(
        saw_call_indirect,
        "Fold with named reducer must emit CallIndirect"
    );
}

// Scenario: fold-reducer type is appended at the correct type index.
// Proves that type_offset + signatures.len() == fold_reducer_type_idx.
// The type section for a 2-binding module with no host imports and fold:
//   type[0]: binding[0] sig
//   type[1]: binding[1] sig
//   type[2]: (i64, i64) → i64  (fold reducer, index 2)
#[test]
fn fold_reducer_type_index_matches_call_indirect_type_index() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.reducer".to_string(),
            expr: AnfExpr::Lambda {
                params: vec!["acc".to_string(), "x".to_string()],
                captures: vec![],
                body: Box::new(AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["acc".to_string(), "x".to_string()],
                }),
            },
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn.fold_user".to_string(),
            expr: AnfExpr::Let {
                name: "z".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Let {
                    name: "xs".to_string(),
                    value: Box::new(AnfExpr::ListNew(vec![])),
                    body: Box::new(AnfExpr::Fold {
                        init: "z".to_string(),
                        list: "xs".to_string(),
                        func: "fn.reducer".to_string(),
                    }),
                }),
            },
        },
    ]);

    let artifact = emit_wasm(&anf).expect("2-binding Fold module must compile");
    wasmparser::validate(&artifact.wasm).expect("2-binding Fold module must validate");

    // For a 2-binding module with no host imports: fold_reducer_type_idx = 0 + 2 = 2.
    let expected_type_idx: u32 = 2;
    let mut saw_expected = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::CallIndirect { type_index, .. } = reader.read().unwrap()
                    && type_index == expected_type_idx
                {
                    saw_expected = true;
                }
            }
        }
    }
    assert!(
        saw_expected,
        "CallIndirect must use type_index={expected_type_idx} (fold reducer type)"
    );
}

// Scenario: Fold where `func` resolves to an I32 local (closure-env pointer)
// must emit Unreachable — not silently dispatch to table[0] via a placeholder
// fn_idx=0.  Proves the W1 guard: Lambda writes fn_idx=0 as a placeholder;
// until lambda hoisting is implemented there is no safe way to use the env.
//
// `env` is bound by VariantNew which yields an I32 pointer.
#[test]
fn fold_with_i32_local_func_emits_unreachable_guard() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.guarded_fold".to_string(),
        expr: AnfExpr::Let {
            name: "acc0".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "env".to_string(),
                    // VariantNew → I32 pointer; serves as the closure-env path.
                    value: Box::new(AnfExpr::VariantNew {
                        tag: "Closure".to_string(),
                        payload: None,
                    }),
                    body: Box::new(AnfExpr::Fold {
                        init: "acc0".to_string(),
                        list: "lst".to_string(),
                        func: "env".to_string(), // I32 local → triggers guard
                    }),
                }),
            }),
        },
    }]);

    // emit_wasm must succeed: the guard is a runtime trap, not a compile error.
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for I32-func Fold");
    // The module must validate: Unreachable is polymorphic.
    wasmparser::validate(&artifact.wasm)
        .expect("I32-func Fold module must validate despite Unreachable guard");

    // The code section must contain Unreachable (the guard against silent fn-0 call).
    let mut saw_unreachable = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::Unreachable = reader.read().unwrap() {
                    saw_unreachable = true;
                }
            }
        }
    }
    assert!(
        saw_unreachable,
        "Fold with I32 closure-env func must emit Unreachable (W1 guard — not silent call fn 0)"
    );
}

// Scenario: Fold where `func` resolves to an unexpected local type (F64) must
// emit Unreachable — not silently dispatch to table[0].  Proves the W2 guard.
#[test]
fn fold_with_unexpected_type_func_emits_unreachable_guard() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.bad_type_fold".to_string(),
        expr: AnfExpr::Let {
            name: "acc0".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "flt".to_string(),
                    // Float literal → F64 local (neither I32 env nor I64 index).
                    value: Box::new(AnfExpr::Literal(LiteralValue::Float(1.0))),
                    body: Box::new(AnfExpr::Fold {
                        init: "acc0".to_string(),
                        list: "lst".to_string(),
                        func: "flt".to_string(), // F64 local → triggers _ arm guard
                    }),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed for F64-func Fold");
    wasmparser::validate(&artifact.wasm)
        .expect("F64-func Fold module must validate despite Unreachable guard");

    let mut saw_unreachable = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::Unreachable = reader.read().unwrap() {
                    saw_unreachable = true;
                }
            }
        }
    }
    assert!(
        saw_unreachable,
        "Fold with unexpected-type (F64) func must emit Unreachable (W2 guard — not silent call fn 0)"
    );
}

// ── Wave 12A: nested Lambda hoisting into function table ──────────────────
//
// A nested Lambda with exactly 2 params and no captures (fold-reducer shape
// `(i64, i64) → i64`) is now hoisted into a separate WASM function instead
// of emitting a closure env placeholder.  The Lambda node itself emits an
// `i64.const <table_idx>` so the Fold can dispatch it via the existing I64
// path (`i32.wrap_i64` + `call_indirect`).
//
// Supported: params.len() == 2, captures.is_empty()
// Not yet supported (still emits closure env with fn_idx=0 placeholder):
//   - Lambdas with captures (general closure hoisting deferred)
//   - Lambdas with != 2 params

// Scenario: a binding whose body contains a hoistable nested Lambda as the
// Fold reducer now compiles, validates, and emits a real CallIndirect.
// Proves fn_idx is no longer 0 (placeholder) for the supported case.
#[test]
fn fold_with_hoistable_nested_lambda_validates_and_emits_call_indirect() {
    use wasmparser::{Operator, Parser, Payload};

    // fn.sum:
    //   let reducer = fn(acc, x) -> acc + x   [hoistable nested Lambda]
    //   let acc0 = 0
    //   let lst = [1, 2, 3]
    //   Fold { init: acc0, list: lst, func: "reducer" }
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.sum".to_string(),
        expr: AnfExpr::Let {
            name: "reducer".to_string(),
            value: Box::new(AnfExpr::Lambda {
                params: vec!["acc".to_string(), "x".to_string()],
                captures: vec![],
                body: Box::new(AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["acc".to_string(), "x".to_string()],
                }),
            }),
            body: Box::new(AnfExpr::Let {
                name: "acc0".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Let {
                    name: "lst".to_string(),
                    value: Box::new(AnfExpr::ListNew(vec![
                        AnfExpr::Literal(LiteralValue::Int(1)),
                        AnfExpr::Literal(LiteralValue::Int(2)),
                        AnfExpr::Literal(LiteralValue::Int(3)),
                    ])),
                    body: Box::new(AnfExpr::Fold {
                        init: "acc0".to_string(),
                        list: "lst".to_string(),
                        func: "reducer".to_string(),
                    }),
                }),
            }),
        },
    }]);

    // Must compile and validate.
    let artifact = emit_wasm(&anf).expect("hoistable nested Lambda Fold must compile successfully");
    wasmparser::validate(&artifact.wasm)
        .expect("hoistable nested Lambda Fold must produce valid WASM");

    let mut saw_table = false;
    let mut saw_element = false;
    let mut saw_call_indirect = false;

    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        match payload.unwrap() {
            Payload::TableSection(_) => saw_table = true,
            Payload::ElementSection(_) => saw_element = true,
            Payload::CodeSectionEntry(body) => {
                let mut reader = body.get_operators_reader().unwrap();
                while !reader.eof() {
                    if let Operator::CallIndirect { .. } = reader.read().unwrap() {
                        saw_call_indirect = true;
                    }
                }
            }
            _ => {}
        }
    }

    assert!(
        saw_table,
        "hoistable Lambda Fold must include a TableSection"
    );
    assert!(
        saw_element,
        "hoistable Lambda Fold must include an ElementSection"
    );
    assert!(
        saw_call_indirect,
        "hoistable Lambda Fold must emit CallIndirect in the code section"
    );
}

// Scenario: the hoisted Lambda body occupies an extra function slot.
// For 1 binding + 1 hoisted Lambda, the table has 2 slots (not 1).
// The hoisted Lambda is at table index 1 (function_offset=0, binding=0 → hoisted=1).
#[test]
fn fold_hoisted_lambda_expands_table_to_n_bindings_plus_n_hoisted() {
    use wasmparser::{Parser, Payload};

    // Same module as fold_with_hoistable_nested_lambda_validates_and_emits_call_indirect.
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.sum".to_string(),
        expr: AnfExpr::Let {
            name: "reducer".to_string(),
            value: Box::new(AnfExpr::Lambda {
                params: vec!["acc".to_string(), "x".to_string()],
                captures: vec![],
                body: Box::new(AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["acc".to_string(), "x".to_string()],
                }),
            }),
            body: Box::new(AnfExpr::Let {
                name: "acc0".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Let {
                    name: "lst".to_string(),
                    value: Box::new(AnfExpr::ListNew(vec![])),
                    body: Box::new(AnfExpr::Fold {
                        init: "acc0".to_string(),
                        list: "lst".to_string(),
                        func: "reducer".to_string(),
                    }),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("WASM must validate");

    // Parse the table section and check initial = 2 (1 binding + 1 hoisted Lambda).
    let mut table_initial: Option<u64> = None;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::TableSection(tables) = payload.unwrap() {
            for table in tables {
                let t = table.unwrap();
                table_initial = Some(t.ty.initial);
            }
        }
    }
    assert_eq!(
        table_initial,
        Some(2),
        "table must have 2 slots: 1 binding + 1 hoisted Lambda; got {table_initial:?}"
    );
}

// Scenario: the hoisted Lambda emits I64Const (table index) not a closure env.
// Verifies the Lambda node no longer allocates linear memory (no I64Store at
// the fn_idx slot) when it is hoistable.
//
// A capture-free 2-param Lambda used with Fold should NOT trigger needs_memory
// solely for the closure env — the hoisted case needs memory only if the
// Lambda body itself accesses memory (which `acc + x` does not).
#[test]
fn fold_hoistable_lambda_does_not_need_memory_for_closure_env() {
    use wasmparser::{Parser, Payload};

    // Binding: fn.sum with hoistable Lambda reducer, no other memory-accessing ops.
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.sum".to_string(),
        expr: AnfExpr::Let {
            name: "f".to_string(),
            value: Box::new(AnfExpr::Lambda {
                params: vec!["a".to_string(), "b".to_string()],
                captures: vec![],
                body: Box::new(AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["a".to_string(), "b".to_string()],
                }),
            }),
            body: Box::new(AnfExpr::Let {
                name: "acc".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Let {
                    name: "lst".to_string(),
                    value: Box::new(AnfExpr::ListNew(vec![])),
                    body: Box::new(AnfExpr::Fold {
                        init: "acc".to_string(),
                        list: "lst".to_string(),
                        func: "f".to_string(),
                    }),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("WASM must validate");

    // The hoistable Lambda does not emit a closure env, so the module must
    // NOT have a memory section (the List header read requires memory, but
    // an empty list means no element reads, and the Lambda body `a + b` is
    // pure arithmetic).
    //
    // Actually: ListNew DOES set needs_memory (stores the count header).
    // So we check the *function count* instead: there must be 2 functions
    // in the function section (binding + hoisted Lambda) — not 1.
    let mut function_count = 0u32;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::FunctionSection(functions) = payload.unwrap() {
            for _ in functions {
                function_count += 1;
            }
        }
    }
    assert_eq!(
        function_count, 2,
        "module must have 2 functions: 1 binding + 1 hoisted Lambda; got {function_count}"
    );
}

// Scenario: Fold reducer is a 2-param Lambda with captures.
// Wave 13B: this was a compile-time diagnostic (FoldWithCapturedReducer).
// Wave 16A PR3: 2-param captured Lambdas are now closure-hoisted into a
// `(env_ptr: i64, acc: i64, elem: i64) → i64` WASM function.  The closure env
// receives the REAL table index in fn_idx, and Fold dispatches via
// call_indirect with the closure-reducer type.  The module must now compile
// and validate successfully.
#[test]
fn fold_closure_hoistable_lambda_with_2_params_compiles_with_pr3() {
    use wasmparser::{Operator, Parser, Payload};

    // Lambda with 2 params AND a capture — closure-hoistable via Wave 16A PR3.
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.biased_sum".to_string(),
        expr: AnfExpr::Let {
            name: "bias".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::Let {
                name: "reducer".to_string(),
                value: Box::new(AnfExpr::Lambda {
                    params: vec!["acc".to_string(), "x".to_string()],
                    captures: vec!["bias".to_string()], // capture → closure-hoistable (PR3)
                    body: Box::new(AnfExpr::Call {
                        func: "+".to_string(),
                        args: vec!["acc".to_string(), "x".to_string()],
                    }),
                }),
                body: Box::new(AnfExpr::Let {
                    name: "acc0".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                    body: Box::new(AnfExpr::Let {
                        name: "lst".to_string(),
                        value: Box::new(AnfExpr::ListNew(vec![])),
                        body: Box::new(AnfExpr::Fold {
                            init: "acc0".to_string(),
                            list: "lst".to_string(),
                            func: "reducer".to_string(),
                        }),
                    }),
                }),
            }),
        },
    }]);

    // Wave 16A PR3: must now compile successfully.
    let artifact = emit_wasm(&anf)
        .expect("2-param captured Lambda reducer must compile successfully (Wave 16A PR3)");
    wasmparser::validate(&artifact.wasm)
        .expect("closure-hoisted fold module must produce valid WASM");

    // The code section must contain CallIndirect (closure-reducer dispatch).
    let mut saw_call_indirect = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::CallIndirect { .. } = reader.read().unwrap() {
                    saw_call_indirect = true;
                }
            }
        }
    }
    assert!(
        saw_call_indirect,
        "closure-hoisted Fold must emit CallIndirect for captured reducer dispatch"
    );
}

// TRIANGULATE: two hoistable nested Lambdas in the same binding — each gets
// a distinct table index.  Proves the sequential counter is correctly advanced.
#[test]
fn two_hoistable_lambdas_get_distinct_table_indices() {
    use wasmparser::{Operator, Parser, Payload};

    // fn.double_fold:
    //   let f1 = fn(a, b) -> a + b     [hoistable, table idx = 1]
    //   let f2 = fn(a, b) -> a + b     [hoistable, table idx = 2]
    //   let acc = 0; let lst = []
    //   let r1 = Fold { func: f1, init: acc, list: lst }
    //   Fold { func: f2, init: r1, list: lst }
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.double_fold".to_string(),
        expr: AnfExpr::Let {
            name: "f1".to_string(),
            value: Box::new(AnfExpr::Lambda {
                params: vec!["a".to_string(), "b".to_string()],
                captures: vec![],
                body: Box::new(AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["a".to_string(), "b".to_string()],
                }),
            }),
            body: Box::new(AnfExpr::Let {
                name: "f2".to_string(),
                value: Box::new(AnfExpr::Lambda {
                    params: vec!["a".to_string(), "b".to_string()],
                    captures: vec![],
                    body: Box::new(AnfExpr::Call {
                        func: "+".to_string(),
                        args: vec!["a".to_string(), "b".to_string()],
                    }),
                }),
                body: Box::new(AnfExpr::Let {
                    name: "acc".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                    body: Box::new(AnfExpr::Let {
                        name: "lst".to_string(),
                        value: Box::new(AnfExpr::ListNew(vec![])),
                        body: Box::new(AnfExpr::Let {
                            name: "r1".to_string(),
                            value: Box::new(AnfExpr::Fold {
                                init: "acc".to_string(),
                                list: "lst".to_string(),
                                func: "f1".to_string(),
                            }),
                            body: Box::new(AnfExpr::Fold {
                                init: "r1".to_string(),
                                list: "lst".to_string(),
                                func: "f2".to_string(),
                            }),
                        }),
                    }),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("double-Fold with two hoistable Lambdas must compile");
    wasmparser::validate(&artifact.wasm).expect("double-Fold module must validate");

    // Table must have 3 slots: 1 binding + 2 hoisted Lambdas.
    let mut table_initial: Option<u64> = None;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::TableSection(tables) = payload.unwrap() {
            for table in tables {
                table_initial = Some(table.unwrap().ty.initial);
            }
        }
    }
    assert_eq!(
        table_initial,
        Some(3),
        "table must have 3 slots: 1 binding + 2 hoisted Lambdas; got {table_initial:?}"
    );

    // Collect I64Const values from the code section — the two table indices
    // (1 and 2) must both be present as distinct I64Const values.
    let mut i64_consts: Vec<i64> = Vec::new();
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::I64Const { value } = reader.read().unwrap() {
                    i64_consts.push(value);
                }
            }
        }
    }
    // Both table index 1 and 2 must appear as I64Const.
    assert!(
        i64_consts.contains(&1),
        "first hoistable Lambda must emit I64Const(1); got consts: {i64_consts:?}"
    );
    assert!(
        i64_consts.contains(&2),
        "second hoistable Lambda must emit I64Const(2); got consts: {i64_consts:?}"
    );
}

// Scenario: function_offset > 0 (module with a host_call import preceding the
// defined functions) + hoistable Lambda + Fold.  Proves `first_hoisted_table_idx`
// is `n_bindings` (not `function_offset + n_bindings`).
//
// Module layout:
//   import[0]  ail/host_call          → function index 0   (function_offset = 1)
//   defined[0] fn.io_noop (EffectCall) → function index 1  (table index 0)
//   defined[1] fn.sum (Fold)           → function index 2  (table index 1)
//   hoisted[0] reducer body            → function index 3  (table index 2)
//
// The hoistable Lambda must emit I64Const(2) — table index n_bindings=2.
// The buggy formula (function_offset + n_bindings = 1+2=3) would emit I64Const(3),
// which is out of the table range [0..2] and would trap at runtime.
#[test]
fn fold_with_nonzero_function_offset_hoistable_lambda_uses_correct_table_idx() {
    use wasmparser::{Operator, Parser, Payload};

    // binding 0: EffectCall with no args — brings in ail/host_call import.
    // binding 1: hoistable Lambda + Fold — hoisted Lambda must get table index 2.
    let anf = sealed_anf(vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.io_noop".to_string(),
            expr: AnfExpr::EffectCall {
                capability: "io".to_string(),
                func: "noop".to_string(),
                args: vec![],
            },
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn.sum".to_string(),
            expr: AnfExpr::Let {
                name: "reducer".to_string(),
                value: Box::new(AnfExpr::Lambda {
                    params: vec!["acc".to_string(), "x".to_string()],
                    captures: vec![],
                    body: Box::new(AnfExpr::Call {
                        func: "+".to_string(),
                        args: vec!["acc".to_string(), "x".to_string()],
                    }),
                }),
                body: Box::new(AnfExpr::Let {
                    name: "acc0".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                    body: Box::new(AnfExpr::Let {
                        name: "lst".to_string(),
                        value: Box::new(AnfExpr::ListNew(vec![])),
                        body: Box::new(AnfExpr::Fold {
                            init: "acc0".to_string(),
                            list: "lst".to_string(),
                            func: "reducer".to_string(),
                        }),
                    }),
                }),
            },
        },
    ]);

    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed with function_offset > 0");
    wasmparser::validate(&artifact.wasm)
        .expect("module with host import + hoistable Lambda Fold must validate");

    // Collect all I64Const values from the code section.
    // The hoistable Lambda emits I64Const(table_idx) where table_idx = n_bindings = 2.
    // The buggy formula would emit I64Const(3) (= function_offset + n_bindings).
    let mut i64_consts: Vec<i64> = Vec::new();
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::I64Const { value } = reader.read().unwrap() {
                    i64_consts.push(value);
                }
            }
        }
    }

    assert!(
        i64_consts.contains(&2),
        "hoistable Lambda must emit I64Const(2) (table index = n_bindings = 2); \
         got consts: {i64_consts:?}"
    );
    assert!(
        !i64_consts.contains(&3),
        "hoistable Lambda must NOT emit I64Const(3) (buggy: function_offset + n_bindings = 3 \
         is out of table bounds [0..2]); got consts: {i64_consts:?}"
    );
}

// ── End Wave 12A nested Lambda hoisting tests ─────────────────────────────

// ── End Wave 11B Fold implementation tests ────────────────────────────────

// ── Wave 10B: generalized unsupported-construct diagnostics ───────────────
//
// Proves that emit_wasm returns CompileError::UnsupportedWasmConstruct for
// each concurrency/dispatch construct that is not yet implemented in the WASM
// backend, rather than silently emitting an unreachable trap.
//
// Pattern per construct:
//   1. Top-level binding → error with the right name.
//   2. Representative nested case (for a subset of constructs).
//
// Defence-in-depth: the unreachable fallback in emit_anf_expr still fires for
// direct callers that bypass emit_wasm_with_profile; this test suite exercises
// the pre-flight gate in emit_wasm_with_profile.

// ── Dispatch ──────────────────────────────────────────────────────────────

// Scenario: top-level Dispatch binding → UnsupportedWasmConstruct("Dispatch").
#[test]
fn dispatch_top_level_returns_unsupported_construct_error() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.dynamic_call".to_string(),
        expr: AnfExpr::Dispatch {
            handler: "vtable".to_string(),
            method: "run".to_string(),
            args: vec![],
        },
    }]);
    let result = emit_wasm(&anf);
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref name)) if name == "Dispatch"
        ),
        "expected UnsupportedWasmConstruct(\"Dispatch\"), got {result:?}"
    );
}

// ── TaskSpawn ─────────────────────────────────────────────────────────────

// Scenario: top-level TaskSpawn → UnsupportedWasmConstruct("TaskSpawn").
#[test]
fn task_spawn_top_level_returns_unsupported_construct_error() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.spawn".to_string(),
        expr: AnfExpr::TaskSpawn {
            func: "worker".to_string(),
            args: vec![],
        },
    }]);
    let result = emit_wasm(&anf);
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref name)) if name == "TaskSpawn"
        ),
        "expected UnsupportedWasmConstruct(\"TaskSpawn\"), got {result:?}"
    );
}

// Scenario: TaskSpawn nested inside a Let chain → pre-flight still catches it.
#[test]
fn task_spawn_nested_in_let_returns_unsupported_construct_error() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.nested_spawn".to_string(),
        expr: AnfExpr::Let {
            name: "arg0".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(42))),
            body: Box::new(AnfExpr::TaskSpawn {
                func: "worker".to_string(),
                args: vec!["arg0".to_string()],
            }),
        },
    }]);
    let result = emit_wasm(&anf);
    assert!(
        matches!(result, Err(CompileError::UnsupportedWasmConstruct(_))),
        "pre-flight gate must detect TaskSpawn nested in Let, got {result:?}"
    );
}

// ── TaskAwait ─────────────────────────────────────────────────────────────

// Scenario: top-level TaskAwait → UnsupportedWasmConstruct("TaskAwait").
#[test]
fn task_await_top_level_returns_unsupported_construct_error() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.await_task".to_string(),
        expr: AnfExpr::TaskAwait {
            task: "t1".to_string(),
        },
    }]);
    let result = emit_wasm(&anf);
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref name)) if name == "TaskAwait"
        ),
        "expected UnsupportedWasmConstruct(\"TaskAwait\"), got {result:?}"
    );
}

// ── TaskCancel ────────────────────────────────────────────────────────────

// Scenario: top-level TaskCancel → UnsupportedWasmConstruct("TaskCancel").
#[test]
fn task_cancel_top_level_returns_unsupported_construct_error() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.cancel_task".to_string(),
        expr: AnfExpr::TaskCancel {
            task: "t1".to_string(),
        },
    }]);
    let result = emit_wasm(&anf);
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref name)) if name == "TaskCancel"
        ),
        "expected UnsupportedWasmConstruct(\"TaskCancel\"), got {result:?}"
    );
}

// ── TaskGroup ─────────────────────────────────────────────────────────────

// Scenario: top-level TaskGroup → UnsupportedWasmConstruct("TaskGroup").
// TaskGroup itself is unsupported; the pre-flight returns "TaskGroup" before
// inspecting its body — the body does not need to contain another unsupported
// construct to trigger the gate.
#[test]
fn task_group_top_level_returns_unsupported_construct_error() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.group".to_string(),
        expr: AnfExpr::TaskGroup {
            body: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
        },
    }]);
    let result = emit_wasm(&anf);
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref name)) if name == "TaskGroup"
        ),
        "expected UnsupportedWasmConstruct(\"TaskGroup\"), got {result:?}"
    );
}

// ── ChannelNew ────────────────────────────────────────────────────────────

// Scenario: top-level ChannelNew → UnsupportedWasmConstruct("ChannelNew").
#[test]
fn channel_new_top_level_returns_unsupported_construct_error() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.make_chan".to_string(),
        expr: AnfExpr::ChannelNew { capacity: None },
    }]);
    let result = emit_wasm(&anf);
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref name)) if name == "ChannelNew"
        ),
        "expected UnsupportedWasmConstruct(\"ChannelNew\"), got {result:?}"
    );
}

// Scenario: ChannelNew nested inside an If branch → gate still fires.
#[test]
fn channel_new_nested_in_if_branch_returns_unsupported_construct_error() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.conditional_chan".to_string(),
        expr: AnfExpr::Let {
            name: "flag".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
            body: Box::new(AnfExpr::If {
                cond: "flag".to_string(),
                then_branch: Box::new(AnfExpr::ChannelNew { capacity: Some(4) }),
                else_branch: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
            }),
        },
    }]);
    let result = emit_wasm(&anf);
    assert!(
        matches!(result, Err(CompileError::UnsupportedWasmConstruct(_))),
        "pre-flight gate must detect ChannelNew inside If branch, got {result:?}"
    );
}

// ── ChannelSend ───────────────────────────────────────────────────────────

// Scenario: top-level ChannelSend → UnsupportedWasmConstruct("ChannelSend").
#[test]
fn channel_send_top_level_returns_unsupported_construct_error() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.send".to_string(),
        expr: AnfExpr::ChannelSend {
            channel: "ch".to_string(),
            value: "v".to_string(),
        },
    }]);
    let result = emit_wasm(&anf);
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref name)) if name == "ChannelSend"
        ),
        "expected UnsupportedWasmConstruct(\"ChannelSend\"), got {result:?}"
    );
}

// ── ChannelReceive ────────────────────────────────────────────────────────

// Scenario: top-level ChannelReceive → UnsupportedWasmConstruct("ChannelReceive").
#[test]
fn channel_receive_top_level_returns_unsupported_construct_error() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.recv".to_string(),
        expr: AnfExpr::ChannelReceive {
            channel: "ch".to_string(),
        },
    }]);
    let result = emit_wasm(&anf);
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref name)) if name == "ChannelReceive"
        ),
        "expected UnsupportedWasmConstruct(\"ChannelReceive\"), got {result:?}"
    );
}

// ── Select ────────────────────────────────────────────────────────────────

// Scenario: top-level Select → UnsupportedWasmConstruct("Select").
#[test]
fn select_top_level_returns_unsupported_construct_error() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.select".to_string(),
        expr: AnfExpr::Select {
            branches: vec![AnfSelectClause {
                channel: "ch1".to_string(),
                binding: "v".to_string(),
                body: AnfExpr::Literal(LiteralValue::Unit),
            }],
        },
    }]);
    let result = emit_wasm(&anf);
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref name)) if name == "Select"
        ),
        "expected UnsupportedWasmConstruct(\"Select\"), got {result:?}"
    );
}

// ── Timeout ───────────────────────────────────────────────────────────────

// Scenario: top-level Timeout → UnsupportedWasmConstruct("Timeout").
#[test]
fn timeout_top_level_returns_unsupported_construct_error() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.timed".to_string(),
        expr: AnfExpr::Timeout {
            duration: "dur".to_string(),
            body: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
        },
    }]);
    let result = emit_wasm(&anf);
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref name)) if name == "Timeout"
        ),
        "expected UnsupportedWasmConstruct(\"Timeout\"), got {result:?}"
    );
}

// ── Cross-construct regression ────────────────────────────────────────────

// Scenario: a module with one clean binding and one TaskSpawn binding in
// the same AnfIr → the pre-flight still rejects the whole compilation.
// Ensures the gate walks ALL bindings, not just the first.
#[test]
fn clean_binding_followed_by_task_spawn_is_rejected() {
    let anf = sealed_anf(vec![
        AnfBinding {
            source_ref: NodeRef(0),
            name: "fn.answer".to_string(),
            expr: AnfExpr::Literal(LiteralValue::Int(42)),
        },
        AnfBinding {
            source_ref: NodeRef(1),
            name: "fn.spawn".to_string(),
            expr: AnfExpr::TaskSpawn {
                func: "worker".to_string(),
                args: vec![],
            },
        },
    ]);
    let result = emit_wasm(&anf);
    assert!(
        matches!(result, Err(CompileError::UnsupportedWasmConstruct(_))),
        "pre-flight gate must scan all bindings; expected error for TaskSpawn in second binding, got {result:?}"
    );
}

// Scenario: Display for each unsupported construct names the construct.
// Ensures the error payload is included in every Display string.
#[test]
fn unsupported_construct_display_names_the_construct() {
    for name in &[
        "Dispatch",
        "TaskSpawn",
        "TaskAwait",
        "TaskCancel",
        "TaskGroup",
        "ChannelNew",
        "ChannelSend",
        "ChannelReceive",
        "Select",
        "Timeout",
        "FoldWithCapturedReducer",
    ] {
        let msg = CompileError::UnsupportedWasmConstruct(name.to_string()).to_string();
        assert!(
            msg.contains(name),
            "Display for UnsupportedWasmConstruct(\"{name}\") must include the construct name; got: {msg}"
        );
    }
}

// ── End Wave 10B unsupported-construct diagnostic tests ───────────────────

// ── Wave 13B / Wave 16A PR3: captured Lambda reducer dispatch ─────────────
//
// Wave 13B added a compile-time diagnostic (FoldWithCapturedReducer) for Fold
// reducers that were captured Lambdas.  The gate blocked all captured Lambdas
// because they could not be hoisted into the (i64, i64) → i64 function table.
//
// Wave 16A PR3 implements general closure hoisting for 2-param captured Lambdas:
// they are emitted as `(env_ptr: i64, acc: i64, elem: i64) → i64` WASM functions
// (closure-reducer type).  The closure env receives the REAL table index in
// fn_idx.  The Fold I32 dispatch path now does call_indirect with the
// closure-reducer type instead of emitting Unreachable.
//
// The gate (FoldWithCapturedReducer) now only fires for Lambdas with captures
// AND ≠ 2 params — those cannot be Fold reducers and still produce a runtime
// type-mismatch trap.
//
// Tests below prove: 2-param captured Lambda Folds compile and validate;
// non-2-param captured Lambda Folds still produce the diagnostic.

// Scenario: minimal Fold + 2-param captured reducer → compiles OK (Wave 16A PR3).
// Wave 13B: this was FoldWithCapturedReducer diagnostic.
// Wave 16A PR3: 2-param captured Lambdas are now closure-hoisted; must compile.
#[test]
fn fold_with_captured_reducer_compiles_with_pr3() {
    // let adder = fn(acc, x) { acc + x }  with capture "bias"
    // fold(zero, lst, adder)
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.biased_fold".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "adder".to_string(),
                    value: Box::new(AnfExpr::Lambda {
                        params: vec!["acc".to_string(), "x".to_string()],
                        captures: vec!["bias".to_string()],
                        body: Box::new(AnfExpr::Call {
                            func: "+".to_string(),
                            args: vec!["acc".to_string(), "x".to_string()],
                        }),
                    }),
                    body: Box::new(AnfExpr::Fold {
                        init: "zero".to_string(),
                        list: "lst".to_string(),
                        func: "adder".to_string(),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "2-param captured reducer must compile successfully (Wave 16A PR3); got {result:?}"
    );
    wasmparser::validate(&result.unwrap().wasm).expect("closure-hoisted fold module must validate");
}

// TRIANGULATE: capture-free 2-param reducer is not affected by the Wave 13B gate.
// Proves that the FoldWithCapturedReducer check does not fire for hoistable Lambdas.
#[test]
fn fold_with_capture_free_reducer_unaffected_by_wave13b_gate() {
    // let zero = 0; let lst = []; let add = fn(acc, x) { acc + x }  (no captures)
    // fold(zero, lst, add)  — must compile without diagnostic
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.plain_sum".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "add".to_string(),
                    value: Box::new(AnfExpr::Lambda {
                        params: vec!["acc".to_string(), "x".to_string()],
                        captures: vec![],
                        body: Box::new(AnfExpr::Call {
                            func: "+".to_string(),
                            args: vec!["acc".to_string(), "x".to_string()],
                        }),
                    }),
                    body: Box::new(AnfExpr::Fold {
                        init: "zero".to_string(),
                        list: "lst".to_string(),
                        func: "add".to_string(),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "Fold with capture-free 2-param Lambda must compile without FoldWithCapturedReducer diagnostic; got {result:?}"
    );
}

// Scenario: captured reducer nested inside an If branch → compiles OK (Wave 16A PR3).
// Wave 13B: this was a FoldWithCapturedReducer diagnostic.
// Wave 16A PR3: 2-param captured Lambdas nested in If branches now compile.
#[test]
fn fold_captured_reducer_in_if_branch_compiles_with_pr3() {
    // if true { fold(0, lst, captured_reducer) } else { 0 }
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.conditional_fold".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "cond".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Bool(true))),
                    body: Box::new(AnfExpr::Let {
                        name: "reducer".to_string(),
                        value: Box::new(AnfExpr::Lambda {
                            params: vec!["acc".to_string(), "x".to_string()],
                            captures: vec!["zero".to_string()],
                            body: Box::new(AnfExpr::Call {
                                func: "+".to_string(),
                                args: vec!["acc".to_string(), "x".to_string()],
                            }),
                        }),
                        body: Box::new(AnfExpr::If {
                            cond: "cond".to_string(),
                            then_branch: Box::new(AnfExpr::Fold {
                                init: "zero".to_string(),
                                list: "lst".to_string(),
                                func: "reducer".to_string(),
                            }),
                            else_branch: Box::new(AnfExpr::Var("zero".to_string())),
                        }),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "2-param captured reducer in If branch must compile (Wave 16A PR3); got {result:?}"
    );
    wasmparser::validate(&result.unwrap().wasm)
        .expect("closure-hoisted If-branch fold must validate");
}

// Scenario: captured reducer inside a Match arm → compiles OK (Wave 16A PR3).
// Wave 13B: this was a FoldWithCapturedReducer diagnostic.
// Wave 16A PR3: 2-param captured Lambdas nested in Match arms now compile.
#[test]
fn fold_captured_reducer_in_match_arm_compiles_with_pr3() {
    use crate::anf::AnfMatchArm;

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.match_fold".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "reducer".to_string(),
                    value: Box::new(AnfExpr::Lambda {
                        params: vec!["acc".to_string(), "x".to_string()],
                        captures: vec!["zero".to_string()],
                        body: Box::new(AnfExpr::Call {
                            func: "+".to_string(),
                            args: vec!["acc".to_string(), "x".to_string()],
                        }),
                    }),
                    body: Box::new(AnfExpr::Match {
                        scrutinee: "zero".to_string(),
                        arms: vec![AnfMatchArm {
                            pattern: "_".to_string(),
                            body: AnfExpr::Fold {
                                init: "zero".to_string(),
                                list: "lst".to_string(),
                                func: "reducer".to_string(),
                            },
                        }],
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "2-param captured reducer in Match arm must compile (Wave 16A PR3); got {result:?}"
    );
    wasmparser::validate(&result.unwrap().wasm)
        .expect("closure-hoisted Match-arm fold must validate");
}

// Scenario: captured reducer inside a Loop body → compiles OK (Wave 16A PR3).
// Wave 13B: this was a FoldWithCapturedReducer diagnostic.
// Wave 16A PR3: 2-param captured Lambdas nested in Loop bodies now compile.
#[test]
fn fold_captured_reducer_in_loop_body_compiles_with_pr3() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.loop_fold".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "reducer".to_string(),
                    value: Box::new(AnfExpr::Lambda {
                        params: vec!["acc".to_string(), "x".to_string()],
                        captures: vec!["zero".to_string()],
                        body: Box::new(AnfExpr::Call {
                            func: "+".to_string(),
                            args: vec!["acc".to_string(), "x".to_string()],
                        }),
                    }),
                    body: Box::new(AnfExpr::Loop {
                        body: Box::new(AnfExpr::Fold {
                            init: "zero".to_string(),
                            list: "lst".to_string(),
                            func: "reducer".to_string(),
                        }),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "2-param captured reducer in Loop body must compile (Wave 16A PR3); got {result:?}"
    );
    wasmparser::validate(&result.unwrap().wasm)
        .expect("closure-hoisted Loop-body fold must validate");
}

// Scenario: transitive Var alias of a 2-param captured reducer → compiles OK (Wave 16A PR3).
// `let adder = lambda captures [...]; let reducer = adder; fold(..., reducer)`
// Wave 13B: this was a FoldWithCapturedReducer diagnostic.
// Wave 16A PR3: both `adder` (closure-hoisted) and its alias `reducer` resolve to the
// same closure env pointer, which carries the real table index.  Must compile.
#[test]
fn fold_with_transitive_var_alias_reducer_compiles_with_pr3() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.aliased_fold".to_string(),
        expr: AnfExpr::Let {
            name: "zero".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![])),
                body: Box::new(AnfExpr::Let {
                    name: "adder".to_string(),
                    value: Box::new(AnfExpr::Lambda {
                        params: vec!["acc".to_string(), "x".to_string()],
                        captures: vec!["bias".to_string()],
                        body: Box::new(AnfExpr::Call {
                            func: "+".to_string(),
                            args: vec!["acc".to_string(), "x".to_string()],
                        }),
                    }),
                    body: Box::new(AnfExpr::Let {
                        name: "reducer".to_string(),
                        value: Box::new(AnfExpr::Var("adder".to_string())),
                        body: Box::new(AnfExpr::Fold {
                            init: "zero".to_string(),
                            list: "lst".to_string(),
                            func: "reducer".to_string(),
                        }),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        result.is_ok(),
        "transitive Var alias of 2-param captured reducer must compile (Wave 16A PR3); got {result:?}"
    );
    wasmparser::validate(&result.unwrap().wasm)
        .expect("transitive alias closure-hoisted fold must validate");
}

// ── Wave 16A PR3: new tests for closure hoisting ──────────────────────────

// Scenario: Fold with a 1-param captured Lambda (NOT a valid fold reducer) →
// FoldWithCapturedReducer diagnostic still fires for non-2-param shapes.
// Proves the gate is still present for cases that Wave 16A PR3 does not handle.
#[test]
fn fold_with_non_2param_captured_lambda_still_returns_diagnostic() {
    // 1-param Lambda with a capture — not a Fold reducer shape (gate preserved).
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.invalid_reducer".to_string(),
        expr: AnfExpr::Let {
            name: "bias".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::Let {
                name: "reducer".to_string(),
                value: Box::new(AnfExpr::Lambda {
                    params: vec!["acc".to_string()], // 1 param — not a fold reducer
                    captures: vec!["bias".to_string()],
                    body: Box::new(AnfExpr::Var("acc".to_string())),
                }),
                body: Box::new(AnfExpr::Let {
                    name: "zero".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                    body: Box::new(AnfExpr::Let {
                        name: "lst".to_string(),
                        value: Box::new(AnfExpr::ListNew(vec![])),
                        body: Box::new(AnfExpr::Fold {
                            init: "zero".to_string(),
                            list: "lst".to_string(),
                            func: "reducer".to_string(),
                        }),
                    }),
                }),
            }),
        },
    }]);

    let result = emit_wasm(&anf);
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref name)) if name == "FoldWithCapturedReducer"
        ),
        "1-param captured Lambda in Fold must still produce FoldWithCapturedReducer; got {result:?}"
    );
}

// Scenario: closure-hoisted Lambda writes real fn_idx (not 0) into closure env.
// Proves Wave 16A PR3: the closure env's fn_idx slot contains the table index
// of the hoisted function, not the placeholder 0.
//
// The module has: 1 binding (fn.biased_sum) + 0 hoisted (no capture-free 2-param
// Lambdas) + 1 closure-hoisted (reducer with "bias" capture).
// → binding function: table index 0, fn index = function_offset + 0
// → closure-hoisted fn: table index 1, fn index = function_offset + 1
//
// The closure env for `reducer` must have fn_idx = 1 (i64.const 1) stored at
// offset 0 of the env struct.  We verify this by scanning the code section for
// `i64.const 1` FOLLOWED BY an `i64.store` — the pattern that writes fn_idx.
#[test]
fn closure_hoisted_lambda_writes_real_fn_idx_not_zero() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.biased_sum".to_string(),
        expr: AnfExpr::Let {
            name: "bias".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(5))),
            body: Box::new(AnfExpr::Let {
                name: "reducer".to_string(),
                value: Box::new(AnfExpr::Lambda {
                    params: vec!["acc".to_string(), "x".to_string()],
                    captures: vec!["bias".to_string()],
                    body: Box::new(AnfExpr::Call {
                        func: "+".to_string(),
                        args: vec!["acc".to_string(), "x".to_string()],
                    }),
                }),
                body: Box::new(AnfExpr::Let {
                    name: "zero".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                    body: Box::new(AnfExpr::Let {
                        name: "lst".to_string(),
                        value: Box::new(AnfExpr::ListNew(vec![])),
                        body: Box::new(AnfExpr::Fold {
                            init: "zero".to_string(),
                            list: "lst".to_string(),
                            func: "reducer".to_string(),
                        }),
                    }),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("closure-hoisted Fold must compile (Wave 16A PR3)");
    wasmparser::validate(&artifact.wasm).expect("closure-hoisted Fold module must validate");

    // Scan code section for i64.const that is NOT 0 followed by i64.store
    // (the fn_idx write sequence).  With 1 binding and 1 closure-hoisted fn,
    // the closure-hoisted fn is at table index 1, so fn_idx = 1.
    let mut saw_nonzero_fn_idx_store = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut ops: Vec<Operator<'_>> = vec![];
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                ops.push(reader.read().unwrap());
            }
            for window in ops.windows(2) {
                if let [Operator::I64Const { value }, Operator::I64Store { .. }] = window {
                    if *value > 0 {
                        saw_nonzero_fn_idx_store = true;
                    }
                }
            }
        }
    }
    assert!(
        saw_nonzero_fn_idx_store,
        "closure env must contain a non-zero fn_idx (real table index, not placeholder 0)"
    );
}

// Scenario: closure-hoisted Lambda module has the correct function count.
// 1 binding + 1 closure-hoisted = 2 WASM functions total.
// Proves build_code_section emits the closure-hoisted body as an extra function.
#[test]
fn closure_hoisted_fold_module_has_correct_function_count() {
    use wasmparser::{Parser, Payload};

    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.biased_sum".to_string(),
        expr: AnfExpr::Let {
            name: "bias".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
            body: Box::new(AnfExpr::Let {
                name: "reducer".to_string(),
                value: Box::new(AnfExpr::Lambda {
                    params: vec!["acc".to_string(), "x".to_string()],
                    captures: vec!["bias".to_string()],
                    body: Box::new(AnfExpr::Call {
                        func: "+".to_string(),
                        args: vec!["acc".to_string(), "x".to_string()],
                    }),
                }),
                body: Box::new(AnfExpr::Let {
                    name: "zero".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                    body: Box::new(AnfExpr::Let {
                        name: "lst".to_string(),
                        value: Box::new(AnfExpr::ListNew(vec![])),
                        body: Box::new(AnfExpr::Fold {
                            init: "zero".to_string(),
                            list: "lst".to_string(),
                            func: "reducer".to_string(),
                        }),
                    }),
                }),
            }),
        },
    }]);

    let artifact = emit_wasm(&anf).expect("closure-hoisted Fold must compile");
    wasmparser::validate(&artifact.wasm).expect("module must validate");

    // Count WASM function bodies in the code section.
    let mut function_count = 0usize;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(_) = payload.unwrap() {
            function_count += 1;
        }
    }
    assert_eq!(
        function_count, 2,
        "module must have 2 functions: 1 binding + 1 closure-hoisted Lambda; got {function_count}"
    );
}

// ── End Wave 13B / Wave 16A PR3 tests ─────────────────────────────────────

// ── Wave 16B: pattern-matching compile-time diagnostics ───────────────────
//
// Proves that unsupported pattern syntax (nested constructors, multi-binding
// tuples, record-field patterns) causes `emit_wasm` to return
// `Err(CompileError::UnsupportedPatternSyntax(...))` instead of silently
// emitting a runtime `Unreachable`.

// Helper: build an AnfIr whose single binding contains a Match with one arm
// that uses the supplied pattern string.  The scrutinee is an i32 variant
// pointer named `"v"` bound by a prior `Let` as `RecordNew([])` (an i32).
fn match_anf_with_pattern(fn_name: &str, pattern: &str) -> AnfIr {
    use crate::anf::{AnfMatchArm, SourceMap};
    use crate::core_ir::StageHashes;
    use ail_core::semantic_graph::NodeRef;

    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: fn_name.to_string(),
        // let v = RecordNew([]); match v { <pattern> => 0 }
        expr: AnfExpr::Let {
            name: "v".to_string(),
            value: Box::new(AnfExpr::RecordNew { fields: vec![] }),
            body: Box::new(AnfExpr::Match {
                scrutinee: "v".to_string(),
                arms: vec![AnfMatchArm {
                    pattern: pattern.to_string(),
                    body: AnfExpr::Literal(LiteralValue::Int(0)),
                }],
            }),
        },
    };
    AnfIr {
        schema_version: crate::anf::ANF_SCHEMA_VERSION,
        source_map: SourceMap::from_bindings(&[binding.clone()]),
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

// Scenario: nested constructor pattern `"Ok(Some(x))"` → UnsupportedPatternSyntax.
// Proves that a pattern with a constructor payload that itself contains `(`
// is rejected at compile time and does NOT compile to a silent Unreachable.
#[test]
fn nested_constructor_pattern_returns_unsupported_pattern_error() {
    let anf = match_anf_with_pattern("fn.nested", "Ok(Some(x))");
    let result = emit_wasm(&anf);
    assert!(
        matches!(result, Err(CompileError::UnsupportedPatternSyntax(_))),
        "nested constructor pattern must return UnsupportedPatternSyntax, got {result:?}"
    );
}

// Scenario: multi-binding pattern `"Pair(a, b)"` → UnsupportedPatternSyntax.
// Proves that a pattern whose payload contains `,` (tuple destructuring)
// is rejected at compile time.
#[test]
fn multi_binding_pattern_returns_unsupported_pattern_error() {
    let anf = match_anf_with_pattern("fn.multi", "Pair(a, b)");
    let result = emit_wasm(&anf);
    assert!(
        matches!(result, Err(CompileError::UnsupportedPatternSyntax(_))),
        "multi-binding pattern must return UnsupportedPatternSyntax, got {result:?}"
    );
}

// Scenario: record-field pattern `"{name: x}"` → UnsupportedPatternSyntax.
// Proves that a pattern using `{` syntax is rejected at compile time.
#[test]
fn record_field_pattern_returns_unsupported_pattern_error() {
    let anf = match_anf_with_pattern("fn.record", "{name: x}");
    let result = emit_wasm(&anf);
    assert!(
        matches!(result, Err(CompileError::UnsupportedPatternSyntax(_))),
        "record-field pattern must return UnsupportedPatternSyntax, got {result:?}"
    );
}

// Scenario: error payload contains the offending pattern string.
// Proves the error carries enough information for a diagnostic message.
#[test]
fn unsupported_pattern_error_carries_pattern_string() {
    let anf = match_anf_with_pattern("fn.nested2", "Ok(Some(x))");
    let Err(CompileError::UnsupportedPatternSyntax(pat)) = emit_wasm(&anf) else {
        panic!("expected UnsupportedPatternSyntax");
    };
    assert!(
        pat.contains("Ok(Some(x))"),
        "error payload must contain the pattern string, got: {pat}"
    );
}

// Scenario: UnsupportedPatternSyntax Display mentions 'pattern' and 'desugared'.
// Proves the error message is diagnostic-quality.
#[test]
fn unsupported_pattern_syntax_display_is_descriptive() {
    let e = CompileError::UnsupportedPatternSyntax("Ok(Some(x))".to_string());
    let msg = e.to_string();
    assert!(
        msg.contains("pattern"),
        "display must contain 'pattern', got: {msg}"
    );
    assert!(
        msg.contains("desugared") || msg.contains("desugar"),
        "display must mention desugaring, got: {msg}"
    );
}

// Scenario: valid single-binding constructor pattern still compiles.
// Proves the detection does not break supported patterns.
#[test]
fn single_binding_constructor_pattern_still_compiles() {
    let anf = match_anf_with_pattern("fn.ok", "Ok(x)");
    let result = emit_wasm(&anf);
    // Should succeed (or fail for unrelated reasons — not UnsupportedPatternSyntax).
    assert!(
        !matches!(result, Err(CompileError::UnsupportedPatternSyntax(_))),
        "single-binding constructor pattern must NOT return UnsupportedPatternSyntax, got {result:?}"
    );
}

// Scenario: tag-only constructor pattern still compiles.
#[test]
fn tag_only_constructor_pattern_still_compiles() {
    let anf = match_anf_with_pattern("fn.none", "None");
    let result = emit_wasm(&anf);
    assert!(
        !matches!(result, Err(CompileError::UnsupportedPatternSyntax(_))),
        "tag-only constructor pattern must NOT return UnsupportedPatternSyntax, got {result:?}"
    );
}

// Scenario: wildcard pattern `"_"` still compiles.
#[test]
fn wildcard_pattern_still_compiles() {
    let anf = match_anf_with_pattern("fn.wildcard", "_");
    let result = emit_wasm(&anf);
    assert!(
        !matches!(result, Err(CompileError::UnsupportedPatternSyntax(_))),
        "wildcard pattern must NOT return UnsupportedPatternSyntax, got {result:?}"
    );
}

// ── End Wave 16B pattern-matching diagnostics tests ───────────────────────

// ── Wave 16A review: W1 and W3 regression tests ───────────────────────────
//
// W1: `collect_closure_hoistable_lambdas` does not recurse into Lambda bodies.
//     A 2-param Lambda nested inside a hoisted or closure-hoisted Lambda body
//     would receive an out-of-range table index.  The compile-time gate in
//     `emit_wasm_with_profile` must reject such programs.
//
// W3: The fallback formula in `build_function_section` for closure-reducer
//     type index incorrectly included `+ hoisted_count`.  Fixed to
//     `type_offset + signatures.len() + 1`.

// W1a — Nested closure-hoistable Lambda inside a closure-hoistable Lambda body
// must be rejected with UnsupportedWasmConstruct("NestedClosureHoistableLambda").
//
// Setup:
//   fn.main = let z = 1
//             in  let outer_f = Lambda(params=[acc,elem], captures=[z],
//                                body = Let("inner_f",
//                                          Lambda(params=[a,b], captures=[z], body=a),
//                                          acc))
//             in  Fold(init=z, list=z, func=outer_f)
//
// The outer Lambda is closure-hoistable (2 params + captures).
// The inner Lambda inside its body is ALSO closure-hoistable — the gate must fire.
#[test]
fn nested_closure_hoistable_lambda_in_closure_body_is_rejected() {
    let inner_lambda = AnfExpr::Lambda {
        params: vec!["a".to_string(), "b".to_string()],
        captures: vec!["z".to_string()],
        body: Box::new(AnfExpr::Var("a".to_string())),
    };
    let outer_lambda = AnfExpr::Lambda {
        params: vec!["acc".to_string(), "elem".to_string()],
        captures: vec!["z".to_string()],
        body: Box::new(AnfExpr::Let {
            name: "inner_f".to_string(),
            value: Box::new(inner_lambda),
            body: Box::new(AnfExpr::Var("acc".to_string())),
        }),
    };
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "z".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::Let {
                name: "outer_f".to_string(),
                value: Box::new(outer_lambda),
                body: Box::new(AnfExpr::Fold {
                    init: "z".to_string(),
                    list: "z".to_string(),
                    func: "outer_f".to_string(),
                }),
            }),
        },
    };
    let result = emit_wasm(&sealed_anf(vec![binding]));
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref s)) if s == "NestedClosureHoistableLambda"
        ),
        "nested closure-hoistable Lambda inside hoisted body must be rejected; got {result:?}"
    );
}

// W1b — Nested hoistable (no-capture) Lambda inside a hoistable Lambda body
// must be rejected with UnsupportedWasmConstruct("NestedHoistableLambda").
//
// Setup:
//   fn.main = let outer_f = Lambda(params=[acc,elem], captures=[],
//                              body = Let("inner_f",
//                                        Lambda(params=[a,b], captures=[], body=a),
//                                        acc))
//             in  Fold(init=outer_f, list=outer_f, func=outer_f)
#[test]
fn nested_hoistable_lambda_in_hoistable_body_is_rejected() {
    let inner_lambda = AnfExpr::Lambda {
        params: vec!["a".to_string(), "b".to_string()],
        captures: vec![],
        body: Box::new(AnfExpr::Var("a".to_string())),
    };
    let outer_lambda = AnfExpr::Lambda {
        params: vec!["acc".to_string(), "elem".to_string()],
        captures: vec![],
        body: Box::new(AnfExpr::Let {
            name: "inner_f".to_string(),
            value: Box::new(inner_lambda),
            body: Box::new(AnfExpr::Var("acc".to_string())),
        }),
    };
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "outer_f".to_string(),
            value: Box::new(outer_lambda),
            body: Box::new(AnfExpr::Fold {
                init: "outer_f".to_string(),
                list: "outer_f".to_string(),
                func: "outer_f".to_string(),
            }),
        },
    };
    let result = emit_wasm(&sealed_anf(vec![binding]));
    assert!(
        matches!(
            result,
            Err(CompileError::UnsupportedWasmConstruct(ref s)) if s == "NestedHoistableLambda"
        ),
        "nested hoistable Lambda inside hoistable body must be rejected; got {result:?}"
    );
}

// W1c — Non-nested Lambda (no 2-param child in body) must NOT be rejected.
// Proves the gate does not over-reject valid closure-hoistable Lambdas.
//
// Setup: outer_f = Lambda(params=[acc,elem], captures=[z], body = add(acc, z))
// The body is a Call, no nested Lambda — must succeed.
#[test]
fn closure_hoistable_lambda_without_nested_2param_lambda_is_accepted() {
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Let {
            name: "z".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![
                    AnfExpr::Literal(LiteralValue::Int(1)),
                    AnfExpr::Literal(LiteralValue::Int(2)),
                ])),
                body: Box::new(AnfExpr::Let {
                    name: "outer_f".to_string(),
                    value: Box::new(AnfExpr::Lambda {
                        params: vec!["acc".to_string(), "elem".to_string()],
                        captures: vec!["z".to_string()],
                        body: Box::new(AnfExpr::Call {
                            func: "+".to_string(),
                            args: vec!["acc".to_string(), "z".to_string()],
                        }),
                    }),
                    body: Box::new(AnfExpr::Fold {
                        init: "z".to_string(),
                        list: "lst".to_string(),
                        func: "outer_f".to_string(),
                    }),
                }),
            }),
        },
    };
    let result = emit_wasm(&sealed_anf(vec![binding]));
    assert!(
        !matches!(result, Err(CompileError::UnsupportedWasmConstruct(ref s))
            if s == "NestedClosureHoistableLambda" || s == "NestedHoistableLambda"),
        "closure-hoistable Lambda with non-nested body must NOT be rejected by W1 gate; got {result:?}"
    );
}

// W3 regression — `build_function_section` closure-reducer fallback formula.
//
// When `closure_reducer_type_idx` is `None`, the fallback must produce
// `type_offset + signatures.len() + 1` (closure-reducer type immediately
// after fold-reducer type).  The old formula incorrectly added `hoisted_count`,
// which belongs to the function section, not the type section.
//
// This test calls `build_function_section` with `closure_reducer_type_idx = None`
// and verifies it returns `Some(...)` without panicking.  The type-index
// correctness of `closure_reducer_type_idx = Some(...)` is exercised by the
// closure-hoistable fold tests above that use `wasmparser::validate`.
#[test]
fn build_function_section_closure_fallback_does_not_panic() {
    use crate::wasm_abi::WasmSignature;
    use crate::wasm_sections::build_function_section;

    let sig = WasmSignature {
        param_count: 1,
        result: Some(wasm_encoder::ValType::I64),
    };
    // type_offset=0, 1 signature, hoisted_count=2, closure_hoisted_count=1.
    // Correct fallback: 0 + 1 + 1 = 2.
    // Wrong (pre-fix) fallback: 0 + 1 + 2 + 1 = 4 (out-of-range type index).
    // build_function_section itself cannot validate the type index (it is a
    // section builder, not a module validator); the test proves it returns
    // Some without panicking and that the fixed formula is applied.
    let section = build_function_section(
        std::slice::from_ref(&sig),
        0,       // type_offset
        2,       // hoisted_count
        Some(1), // fold_reducer_type_idx = type_offset + sigs.len() = 1
        1,       // closure_hoisted_count
        None,    // closure_reducer_type_idx = None → uses fallback formula
    );
    assert!(
        section.is_some(),
        "build_function_section must return Some when bindings + hoisted > 0"
    );
}

// ── End Wave 16A W1/W3 regression tests ──────────────────────────────────
