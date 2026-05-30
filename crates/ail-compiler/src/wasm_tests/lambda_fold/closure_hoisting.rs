use super::*;

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
                if let [Operator::I64Const { value }, Operator::I64Store { .. }] = window
                    && *value > 0
                {
                    saw_nonzero_fn_idx_store = true;
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

