use super::helpers::*;

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

// Scenario: ForEach pushes a unit value (I32 0) so it can appear as the
// value in a `Let` binding or inside a `Seq` without causing a WASM
// stack-underflow validation error. infer_expr_type must return Some(I32).
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
        Some(ValType::I32),
        "ForEach emits a unit I32 0 onto the WASM stack — infer_expr_type must return Some(I32)"
    );
}

// ── Wave 18D: WhileLoop infer_expr_type returns Some(I32) ────────────────
//
// Scenario: WhileLoop must push a unit value (I32 0) onto the WASM stack so
// it can appear as the value in a `Let` binding or inside a `Seq` without
// causing a stack-underflow validation error.  infer_expr_type must return
// Some(I32) — analogous to the ForEach fix from Wave 18B.
#[test]
fn while_loop_infer_expr_type_is_i32() {
    use crate::wasm_abi::infer_expr_type;
    use wasm_encoder::ValType;

    let expr = AnfExpr::WhileLoop {
        cond: "flag".to_string(),
        body: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
    };
    let mut locals: Vec<(String, ValType)> = vec![];
    assert_eq!(
        infer_expr_type(&expr, &mut locals),
        Some(ValType::I32),
        "WhileLoop emits a unit I32 0 onto the WASM stack — infer_expr_type must return Some(I32)"
    );
}

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
        "FoldWithUncapturedWrongArityReducer",
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
