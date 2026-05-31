// ── ail-compiler::wasm_text_literal_tests ────────────────────────────────
//
// TDD RED phase — written before Text literal linear-memory encoding exists.
//
// Spec scenarios covered (C-1a, C-1b, C-1c):
//  - Emit WASM with Literal(Text("hello")) → data section non-empty.
//  - i64 value packs (len as u32) << 32 | (ptr as u32) correctly.
//  - Two different text literals produce different i64 packed values.

use ail_compiler::core_ir::{LiteralValue, StageHashes};
use ail_compiler::{AnfBinding, AnfExpr, AnfIr, SourceMap, WasmTypeDescriptor, emit_wasm};
use ail_core::semantic_graph::NodeRef;
use wasmparser::{Parser, Payload};

// ── helpers ──────────────────────────────────────────────────────────────

fn sealed_anf(binding: AnfBinding) -> AnfIr {
    AnfIr {
        schema_version: ail_compiler::anf::ANF_SCHEMA_VERSION,
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

fn emit_text_literal_wasm(text: &str) -> Vec<u8> {
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Text(text.to_string())),
    };
    let artifact = emit_wasm(&sealed_anf(binding)).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("emitted WASM must validate");
    artifact.wasm
}

fn emit_text_expr(name: &str, expr: AnfExpr) -> ail_compiler::WasmArtifact {
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: name.to_string(),
        expr,
    };
    let artifact = emit_wasm(&sealed_anf(binding)).expect("emit_wasm must succeed");
    wasmparser::validate(&artifact.wasm).expect("emitted WASM must validate");
    artifact
}

/// Count the number of data section entries in a WASM binary.
fn data_section_entry_count(wasm: &[u8]) -> usize {
    let mut count = 0;
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::DataSection(reader) = payload.expect("payload must parse") {
            for segment in reader {
                let _ = segment.expect("data segment");
                count += 1;
            }
        }
    }
    count
}

/// Check whether the WASM binary has a data section with at least one entry.
fn has_data_section(wasm: &[u8]) -> bool {
    data_section_entry_count(wasm) > 0
}

/// Extract all i64.const values from the code section.
fn i64_const_values(wasm: &[u8]) -> Vec<i64> {
    let mut values = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::CodeSectionEntry(body) = payload.expect("payload must parse") {
            let mut reader = body.get_operators_reader().expect("operators reader");
            while !reader.eof() {
                if let wasmparser::Operator::I64Const { value } =
                    reader.read().expect("operator must read")
                {
                    values.push(value);
                }
            }
        }
    }
    values
}

// ── Scenario C-1a: data section is non-empty ──────────────────────────────

#[test]
fn text_literal_produces_data_section() {
    let wasm = emit_text_literal_wasm("hello");
    assert!(
        has_data_section(&wasm),
        "WASM with Text literal must have a non-empty data section"
    );
}

// ── Scenario C-1b: i64 packs ptr and len correctly ───────────────────────

#[test]
fn text_literal_i64_packs_ptr_and_len() {
    let text = "hello";
    let wasm = emit_text_literal_wasm(text);

    // The i64 constant should encode: (len << 32) | ptr
    let values = i64_const_values(&wasm);
    assert!(
        !values.is_empty(),
        "must have at least one i64.const in code section"
    );

    // Find a value whose upper 32 bits = len (ptr can be 0 for the first/only string)
    let expected_len = text.len() as i64;
    let packed = values.iter().find(|&&v| {
        let len_part = (v as u64) >> 32;
        len_part == expected_len as u64
    });

    assert!(
        packed.is_some(),
        "must find i64 encoding with len={expected_len} in upper 32 bits, got values: {values:?}"
    );
}

// ── Scenario C-1c: different texts produce different i64 values ───────────
// Uses different-length strings to guarantee different upper 32 bits.

#[test]
fn two_different_text_literals_produce_different_i64() {
    // "hi" (len=2) vs "hello" (len=5) — different lengths → different upper 32 bits
    let wasm_hi = emit_text_literal_wasm("hi");
    let wasm_hello = emit_text_literal_wasm("hello");

    let values_hi = i64_const_values(&wasm_hi);
    let values_hello = i64_const_values(&wasm_hello);

    assert_ne!(
        values_hi, values_hello,
        "different-length text literals must produce different i64 packed values"
    );
}

// ── Bonus: text-returning function now exports (formerly i32 placeholder) ─

#[test]
fn text_literal_function_is_exported() {
    let wasm = emit_text_literal_wasm("hi");

    // After the fix, Text literals return I64 → binding_result returns Some(I64) → exported
    let mut export_names = Vec::new();
    for payload in Parser::new(0).parse_all(&wasm) {
        if let Payload::ExportSection(reader) = payload.expect("payload") {
            for export in reader {
                let e = export.expect("export");
                export_names.push(e.name.to_string());
            }
        }
    }

    assert!(
        export_names.iter().any(|n| n == "main"),
        "Text-returning function must be exported as 'main', got {export_names:?}"
    );
}

#[test]
fn string_len_call_exports_scalar_wasm() {
    let artifact = emit_text_expr(
        "fn.main",
        AnfExpr::Let {
            name: "text".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text("hello".to_string()))),
            body: Box::new(AnfExpr::Call {
                func: "len".to_string(),
                args: vec!["text".to_string()],
            }),
        },
    );

    assert!(
        artifact.export_types.contains_key("main"),
        "len(Text) must leave an exported scalar result"
    );
}

#[test]
fn string_concat_call_preserves_text_export_type() {
    let artifact = emit_text_expr(
        "fn.main",
        AnfExpr::Let {
            name: "left".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text("he".to_string()))),
            body: Box::new(AnfExpr::Let {
                name: "right".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Text("llo".to_string()))),
                body: Box::new(AnfExpr::Call {
                    func: "concat".to_string(),
                    args: vec!["left".to_string(), "right".to_string()],
                }),
            }),
        },
    );

    assert_eq!(
        artifact.export_types.get("main"),
        Some(&WasmTypeDescriptor::Text),
        "concat(Text, Text) must preserve the public Text ABI descriptor"
    );
}

#[test]
fn string_trim_call_preserves_text_export_type() {
    let artifact = emit_text_expr(
        "fn.main",
        AnfExpr::Let {
            name: "value".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text(" hello ".to_string()))),
            body: Box::new(AnfExpr::Call {
                func: "text.trim".to_string(),
                args: vec!["value".to_string()],
            }),
        },
    );

    assert_eq!(
        artifact.export_types.get("main"),
        Some(&WasmTypeDescriptor::Text),
        "text.trim(Text) must preserve the public Text ABI descriptor"
    );
}

#[test]
fn string_slice_call_preserves_text_export_type() {
    let artifact = emit_text_expr(
        "fn.main",
        AnfExpr::Let {
            name: "value".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text("hello".to_string()))),
            body: Box::new(AnfExpr::Let {
                name: "start".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                body: Box::new(AnfExpr::Let {
                    name: "length".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(3))),
                    body: Box::new(AnfExpr::Call {
                        func: "text.slice".to_string(),
                        args: vec![
                            "value".to_string(),
                            "start".to_string(),
                            "length".to_string(),
                        ],
                    }),
                }),
            }),
        },
    );

    assert_eq!(
        artifact.export_types.get("main"),
        Some(&WasmTypeDescriptor::Text),
        "text.slice(Text, Int, Int) must preserve the public Text ABI descriptor"
    );
}

#[test]
fn string_slice_emits_utf8_boundary_gate() {
    let artifact = emit_text_expr(
        "fn.main",
        AnfExpr::Let {
            name: "value".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text("éé".to_string()))),
            body: Box::new(AnfExpr::Let {
                name: "start".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Let {
                    name: "length".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                    body: Box::new(AnfExpr::Call {
                        func: "text.slice".to_string(),
                        args: vec![
                            "value".to_string(),
                            "start".to_string(),
                            "length".to_string(),
                        ],
                    }),
                }),
            }),
        },
    );

    let mut saw_continuation_mask = false;
    let mut saw_continuation_tag = false;
    let mut saw_and = false;

    for payload in Parser::new(0).parse_all(&artifact.wasm) {
        if let Payload::CodeSectionEntry(body) = payload.expect("payload must parse") {
            let mut reader = body.get_operators_reader().expect("operators reader");
            while !reader.eof() {
                match reader.read().expect("operator must read") {
                    wasmparser::Operator::I32Const { value: 0xC0 } => {
                        saw_continuation_mask = true;
                    }
                    wasmparser::Operator::I32Const { value: 0x80 } => {
                        saw_continuation_tag = true;
                    }
                    wasmparser::Operator::I32And => {
                        saw_and = true;
                    }
                    _ => {}
                }
            }
        }
    }

    assert!(
        saw_continuation_mask && saw_continuation_tag && saw_and,
        "text.slice must gate UTF-8 continuation-byte boundaries before copying"
    );
}

#[test]
fn string_replace_first_call_preserves_text_export_type() {
    let artifact = emit_text_expr(
        "fn.main",
        AnfExpr::Let {
            name: "value".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text("hello".to_string()))),
            body: Box::new(AnfExpr::Let {
                name: "needle".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Text("ell".to_string()))),
                body: Box::new(AnfExpr::Let {
                    name: "replacement".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Text("ipp".to_string()))),
                    body: Box::new(AnfExpr::Call {
                        func: "text.replace_first".to_string(),
                        args: vec![
                            "value".to_string(),
                            "needle".to_string(),
                            "replacement".to_string(),
                        ],
                    }),
                }),
            }),
        },
    );

    assert_eq!(
        artifact.export_types.get("main"),
        Some(&WasmTypeDescriptor::Text),
        "text.replace_first(Text, Text, Text) must preserve the public Text ABI descriptor"
    );
}

#[test]
fn string_eq_call_exports_scalar_wasm() {
    let artifact = emit_text_expr(
        "fn.main",
        AnfExpr::Let {
            name: "left".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text("hello".to_string()))),
            body: Box::new(AnfExpr::Let {
                name: "right".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Text("hello".to_string()))),
                body: Box::new(AnfExpr::Call {
                    func: "text.eq".to_string(),
                    args: vec!["left".to_string(), "right".to_string()],
                }),
            }),
        },
    );

    assert!(
        artifact.export_types.contains_key("main"),
        "text.eq(Text, Text) must leave an exported scalar result"
    );
}

#[test]
fn string_contains_call_exports_scalar_wasm() {
    let artifact = emit_text_expr(
        "fn.main",
        AnfExpr::Let {
            name: "haystack".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text("hello".to_string()))),
            body: Box::new(AnfExpr::Let {
                name: "needle".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Text("ell".to_string()))),
                body: Box::new(AnfExpr::Call {
                    func: "text.contains".to_string(),
                    args: vec!["haystack".to_string(), "needle".to_string()],
                }),
            }),
        },
    );

    assert!(
        artifact.export_types.contains_key("main"),
        "text.contains(Text, Text) must leave an exported scalar result"
    );
}

#[test]
fn string_index_of_call_exports_scalar_wasm() {
    let artifact = emit_text_expr(
        "fn.main",
        AnfExpr::Let {
            name: "haystack".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text("hello".to_string()))),
            body: Box::new(AnfExpr::Let {
                name: "needle".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Text("ell".to_string()))),
                body: Box::new(AnfExpr::Call {
                    func: "text.index_of".to_string(),
                    args: vec!["haystack".to_string(), "needle".to_string()],
                }),
            }),
        },
    );

    assert!(
        artifact.export_types.contains_key("main"),
        "text.index_of(Text, Text) must leave an exported scalar result"
    );
}

#[test]
fn string_byte_at_or_call_exports_scalar_wasm() {
    let artifact = emit_text_expr(
        "fn.main",
        AnfExpr::Let {
            name: "value".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text("hello".to_string()))),
            body: Box::new(AnfExpr::Let {
                name: "index".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
                body: Box::new(AnfExpr::Let {
                    name: "fallback".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(-1))),
                    body: Box::new(AnfExpr::Call {
                        func: "text.byte_at_or".to_string(),
                        args: vec![
                            "value".to_string(),
                            "index".to_string(),
                            "fallback".to_string(),
                        ],
                    }),
                }),
            }),
        },
    );

    assert!(
        artifact.export_types.contains_key("main"),
        "text.byte_at_or(Text, Int, Int) must leave an exported scalar result"
    );
}

#[test]
fn string_parse_int_or_call_exports_scalar_wasm() {
    let artifact = emit_text_expr(
        "fn.main",
        AnfExpr::Let {
            name: "value".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text("-42".to_string()))),
            body: Box::new(AnfExpr::Let {
                name: "fallback".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
                body: Box::new(AnfExpr::Call {
                    func: "text.parse_int_or".to_string(),
                    args: vec!["value".to_string(), "fallback".to_string()],
                }),
            }),
        },
    );

    assert!(
        artifact.export_types.contains_key("main"),
        "text.parse_int_or(Text, Int) must leave an exported scalar result"
    );
}

#[test]
fn string_starts_with_call_exports_scalar_wasm() {
    let artifact = emit_text_expr(
        "fn.main",
        AnfExpr::Let {
            name: "haystack".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text("hello".to_string()))),
            body: Box::new(AnfExpr::Let {
                name: "prefix".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Text("he".to_string()))),
                body: Box::new(AnfExpr::Call {
                    func: "text.starts_with".to_string(),
                    args: vec!["haystack".to_string(), "prefix".to_string()],
                }),
            }),
        },
    );

    assert!(
        artifact.export_types.contains_key("main"),
        "text.starts_with(Text, Text) must leave an exported scalar result"
    );
}

#[test]
fn string_ends_with_call_exports_scalar_wasm() {
    let artifact = emit_text_expr(
        "fn.main",
        AnfExpr::Let {
            name: "haystack".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Text("hello".to_string()))),
            body: Box::new(AnfExpr::Let {
                name: "suffix".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Text("lo".to_string()))),
                body: Box::new(AnfExpr::Call {
                    func: "text.ends_with".to_string(),
                    args: vec!["haystack".to_string(), "suffix".to_string()],
                }),
            }),
        },
    );

    assert!(
        artifact.export_types.contains_key("main"),
        "text.ends_with(Text, Text) must leave an exported scalar result"
    );
}
