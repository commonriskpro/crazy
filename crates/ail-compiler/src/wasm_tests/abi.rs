use super::helpers::*;

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
fn wasm_type_let_bound_text_var_is_text() {
    let expr = AnfExpr::Let {
        name: "s".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Text(
            "Hello, world!".to_string(),
        ))),
        body: Box::new(AnfExpr::Var("s".to_string())),
    };

    assert_eq!(derive_wasm_type(&expr), WasmTypeDescriptor::Text);
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

#[test]
fn emitted_text_export_abi_descriptor_accepts_matching_module_boundary() {
    let anf = sealed_anf(vec![AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.greeting".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Text("hello".to_string())),
    }]);

    let artifact = emit_wasm(&anf).unwrap();
    wasmparser::validate(&artifact.wasm).expect("wasm must validate");

    let mut module = crate::wasm_abi::AbiModuleShape::new(std::collections::BTreeMap::from([(
        "greeting".to_string(),
        crate::wasm_abi::AbiFunctionSignature::new(vec![], Some(WasmScalarType::I64)),
    )]));
    module.memory_exported = true;

    assert_eq!(
        artifact.export_types.get("greeting"),
        Some(&WasmTypeDescriptor::Text)
    );
    assert_eq!(
        artifact
            .abi_descriptor
            .validation_issues_for_module(&module),
        vec![]
    );
    assert_eq!(
        artifact
            .abi_descriptor
            .validation_diagnostics_for_module(&module),
        vec![]
    );
}
