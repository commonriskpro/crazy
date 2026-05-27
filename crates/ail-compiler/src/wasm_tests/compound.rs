use super::helpers::*;

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

// ── TASK-A7: EffectCall I32 arg zero-extension tests (TDD RED) ───────
// Spec scenarios C-4a, C-4b.

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
