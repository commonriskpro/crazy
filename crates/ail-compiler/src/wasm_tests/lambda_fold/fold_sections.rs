use super::*;

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
