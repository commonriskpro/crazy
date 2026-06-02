use super::helpers::*;

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

fn file_read_bytes_effect_call_anf() -> AnfIr {
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.read_file".to_string(),
        expr: AnfExpr::Let {
            name: "path".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text("data.bin".to_string()))),
            body: Box::new(AnfExpr::EffectCall {
                capability: "file.read".to_string(),
                func: "read".to_string(),
                args: vec!["path".to_string()],
            }),
        },
    };
    sealed_anf(vec![binding])
}

#[test]
fn file_read_bytes_effect_call_uses_host_call_write_buffer() {
    use wasmparser::{Operator, Parser, Payload};

    let anf = file_read_bytes_effect_call_anf();
    let layout = EffectDataLayout::for_bindings(&anf.bindings);
    assert!(
        layout.needs_host_call_write,
        "file.read/read returns Bytes and must use host_call_write"
    );

    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    let mut saw_host_call_write = false;
    let mut saw_pack_or = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                match reader.read().unwrap() {
                    Operator::Call { function_index: 1 } => saw_host_call_write = true,
                    Operator::I64Or => saw_pack_or = true,
                    _ => {}
                }
            }
        }
    }

    assert!(
        saw_host_call_write,
        "file.read/read must call host_call_write"
    );
    assert!(
        saw_pack_or,
        "file.read/read must pack result ptr/len for Bytes ABI"
    );
}

#[test]
fn bytes_length_of_file_read_uses_packed_length() {
    use wasmparser::{Operator, Parser, Payload};

    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.read_file_len".to_string(),
        expr: AnfExpr::Let {
            name: "path".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text("data.bin".to_string()))),
            body: Box::new(AnfExpr::Let {
                name: "data".to_string(),
                value: Box::new(AnfExpr::EffectCall {
                    capability: "file.read".to_string(),
                    func: "read".to_string(),
                    args: vec!["path".to_string()],
                }),
                body: Box::new(AnfExpr::Call {
                    func: "std.bytes.length".to_string(),
                    args: vec!["data".to_string()],
                }),
            }),
        },
    };
    let artifact = emit_wasm(&sealed_anf(vec![binding])).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    let mut saw_len_shift = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                if let Operator::I64ShrU = reader.read().unwrap() {
                    saw_len_shift = true;
                }
            }
        }
    }

    assert!(
        saw_len_shift,
        "std.bytes.length must read len from packed Bytes high bits"
    );
}

#[test]
fn bytes_empty_of_file_read_compares_packed_length_to_zero() {
    use wasmparser::{Operator, Parser, Payload};

    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.read_file_empty".to_string(),
        expr: AnfExpr::Let {
            name: "path".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text(
                "empty.bin".to_string(),
            ))),
            body: Box::new(AnfExpr::Let {
                name: "data".to_string(),
                value: Box::new(AnfExpr::EffectCall {
                    capability: "file.read".to_string(),
                    func: "read".to_string(),
                    args: vec!["path".to_string()],
                }),
                body: Box::new(AnfExpr::Call {
                    func: "std.bytes.empty".to_string(),
                    args: vec!["data".to_string()],
                }),
            }),
        },
    };
    let artifact = emit_wasm(&sealed_anf(vec![binding])).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    let mut saw_len_shift = false;
    let mut saw_eqz = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                match reader.read().unwrap() {
                    Operator::I64ShrU => saw_len_shift = true,
                    Operator::I64Eqz => saw_eqz = true,
                    _ => {}
                }
            }
        }
    }

    assert!(
        saw_len_shift,
        "std.bytes.empty must read len from packed Bytes high bits"
    );
    assert!(saw_eqz, "std.bytes.empty must compare byte length to zero");
}

#[test]
fn bytes_at_of_file_read_emits_option_byte_load() {
    use wasmparser::{Operator, Parser, Payload};

    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.read_file_byte".to_string(),
        expr: AnfExpr::Let {
            name: "path".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text(
                "payload.bin".to_string(),
            ))),
            body: Box::new(AnfExpr::Let {
                name: "data".to_string(),
                value: Box::new(AnfExpr::EffectCall {
                    capability: "file.read".to_string(),
                    func: "read".to_string(),
                    args: vec!["path".to_string()],
                }),
                body: Box::new(AnfExpr::Let {
                    name: "index".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                    body: Box::new(AnfExpr::Call {
                        func: "std.bytes.at".to_string(),
                        args: vec!["data".to_string(), "index".to_string()],
                    }),
                }),
            }),
        },
    };
    let anf = sealed_anf(vec![binding]);
    let layout = EffectDataLayout::for_bindings(&anf.bindings);
    assert!(
        layout.needs_memory,
        "std.bytes.at must allocate the returned Option variant"
    );
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    let mut saw_len_shift = false;
    let mut saw_byte_load = false;
    let mut saw_some_tag = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                match reader.read().unwrap() {
                    Operator::I64ShrU => saw_len_shift = true,
                    Operator::I32Load8U { .. } => saw_byte_load = true,
                    Operator::I32Const { value: 1 } => saw_some_tag = true,
                    _ => {}
                }
            }
        }
    }

    assert!(
        saw_len_shift,
        "std.bytes.at must read len from packed Bytes high bits"
    );
    assert!(saw_byte_load, "std.bytes.at must load one byte from memory");
    assert!(saw_some_tag, "std.bytes.at must write the Some tag on hit");
}

#[test]
fn bytes_slice_of_file_read_emits_option_copy() {
    use wasmparser::{Operator, Parser, Payload};

    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.read_file_slice".to_string(),
        expr: AnfExpr::Let {
            name: "path".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text(
                "payload.bin".to_string(),
            ))),
            body: Box::new(AnfExpr::Let {
                name: "data".to_string(),
                value: Box::new(AnfExpr::EffectCall {
                    capability: "file.read".to_string(),
                    func: "read".to_string(),
                    args: vec!["path".to_string()],
                }),
                body: Box::new(AnfExpr::Let {
                    name: "start".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                    body: Box::new(AnfExpr::Let {
                        name: "end".to_string(),
                        value: Box::new(AnfExpr::Literal(LiteralValue::Int(4))),
                        body: Box::new(AnfExpr::Call {
                            func: "std.bytes.slice".to_string(),
                            args: vec!["data".to_string(), "start".to_string(), "end".to_string()],
                        }),
                    }),
                }),
            }),
        },
    };
    let anf = sealed_anf(vec![binding]);
    let layout = EffectDataLayout::for_bindings(&anf.bindings);
    assert!(
        layout.needs_memory,
        "std.bytes.slice must allocate the returned Option<Bytes> variant and slice payload"
    );
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    let mut saw_len_shift = false;
    let mut saw_memory_copy = false;
    let mut saw_pack_or = false;
    let mut saw_some_tag = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                match reader.read().unwrap() {
                    Operator::I64ShrU => saw_len_shift = true,
                    Operator::MemoryCopy { .. } => saw_memory_copy = true,
                    Operator::I64Or => saw_pack_or = true,
                    Operator::I32Const { value: 1 } => saw_some_tag = true,
                    _ => {}
                }
            }
        }
    }

    assert!(
        saw_len_shift,
        "std.bytes.slice must read len from packed Bytes high bits"
    );
    assert!(
        saw_memory_copy,
        "std.bytes.slice must copy the selected byte range"
    );
    assert!(
        saw_pack_or,
        "std.bytes.slice must pack slice len/ptr payload"
    );
    assert!(
        saw_some_tag,
        "std.bytes.slice must write the Some tag on hit"
    );
}

#[test]
fn bytes_concat_of_file_reads_emits_two_copies_and_packs_bytes() {
    use wasmparser::{Operator, Parser, Payload};

    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.read_file_concat".to_string(),
        expr: AnfExpr::Let {
            name: "left_path".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text("left.bin".to_string()))),
            body: Box::new(AnfExpr::Let {
                name: "right_path".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Text(
                    "right.bin".to_string(),
                ))),
                body: Box::new(AnfExpr::Let {
                    name: "left".to_string(),
                    value: Box::new(AnfExpr::EffectCall {
                        capability: "file.read".to_string(),
                        func: "read".to_string(),
                        args: vec!["left_path".to_string()],
                    }),
                    body: Box::new(AnfExpr::Let {
                        name: "right".to_string(),
                        value: Box::new(AnfExpr::EffectCall {
                            capability: "file.read".to_string(),
                            func: "read".to_string(),
                            args: vec!["right_path".to_string()],
                        }),
                        body: Box::new(AnfExpr::Call {
                            func: "std.bytes.concat".to_string(),
                            args: vec!["left".to_string(), "right".to_string()],
                        }),
                    }),
                }),
            }),
        },
    };
    let anf = sealed_anf(vec![binding]);
    let layout = EffectDataLayout::for_bindings(&anf.bindings);
    assert!(
        layout.needs_memory,
        "std.bytes.concat must allocate the merged Bytes payload"
    );
    let artifact = emit_wasm(&anf).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    let mut memory_copy_count = 0usize;
    let mut saw_len_add = false;
    let mut saw_pack_or = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                match reader.read().unwrap() {
                    Operator::MemoryCopy { .. } => memory_copy_count += 1,
                    Operator::I32Add => saw_len_add = true,
                    Operator::I64Or => saw_pack_or = true,
                    _ => {}
                }
            }
        }
    }

    assert!(
        memory_copy_count >= 2,
        "std.bytes.concat must copy both left and right buffers"
    );
    assert!(saw_len_add, "std.bytes.concat must add both byte lengths");
    assert!(saw_pack_or, "std.bytes.concat must pack merged len/ptr");
}

#[test]
fn time_arithmetic_after_clock_effect_emits_add_and_sub() {
    use wasmparser::{Operator, Parser, Payload};

    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.time_delta".to_string(),
        expr: AnfExpr::Let {
            name: "now".to_string(),
            value: Box::new(AnfExpr::EffectCall {
                capability: "clock.now".to_string(),
                func: "now".to_string(),
                args: vec![],
            }),
            body: Box::new(AnfExpr::Let {
                name: "delta".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(250))),
                body: Box::new(AnfExpr::Let {
                    name: "later".to_string(),
                    value: Box::new(AnfExpr::Call {
                        func: "std.time.add_duration".to_string(),
                        args: vec!["now".to_string(), "delta".to_string()],
                    }),
                    body: Box::new(AnfExpr::Call {
                        func: "std.time.duration_since".to_string(),
                        args: vec!["later".to_string(), "now".to_string()],
                    }),
                }),
            }),
        },
    };
    let artifact = emit_wasm(&sealed_anf(vec![binding])).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    let mut saw_add = false;
    let mut saw_sub = false;
    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                match reader.read().unwrap() {
                    Operator::I64Add => saw_add = true,
                    Operator::I64Sub => saw_sub = true,
                    _ => {}
                }
            }
        }
    }

    assert!(saw_add, "std.time.add_duration must emit I64Add");
    assert!(saw_sub, "std.time.duration_since must emit I64Sub");
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
