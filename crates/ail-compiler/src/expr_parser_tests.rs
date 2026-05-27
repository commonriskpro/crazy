// Tests for the expression parser.
// Declared from expr_parser.rs as: #[cfg(test)] #[path = "expr_parser_tests.rs"] mod tests;

use crate::core_ir::MatchArm;

use super::*;

#[test]
fn parses_add_call_to_core_expr_add() {
    assert_eq!(
        parse_expr("add(x, y)").unwrap(),
        CoreExpr::Add(
            Box::new(CoreExpr::Var("x".to_string())),
            Box::new(CoreExpr::Var("y".to_string()))
        )
    );
}

#[test]
fn parses_nested_sum_of_squares() {
    assert_eq!(
        parse_expr("add(mul(x, x), mul(y, y))").unwrap(),
        CoreExpr::Add(
            Box::new(CoreExpr::Mul(
                Box::new(CoreExpr::Var("x".to_string())),
                Box::new(CoreExpr::Var("x".to_string()))
            )),
            Box::new(CoreExpr::Mul(
                Box::new(CoreExpr::Var("y".to_string())),
                Box::new(CoreExpr::Var("y".to_string()))
            ))
        )
    );
}

#[test]
fn parses_let_binding() {
    assert_eq!(
        parse_expr("let(total, add(x, y), if(gt(total, 10), total, 0))").unwrap(),
        CoreExpr::Let {
            name: "total".to_string(),
            value: Box::new(CoreExpr::Add(
                Box::new(CoreExpr::Var("x".to_string())),
                Box::new(CoreExpr::Var("y".to_string()))
            )),
            body: Box::new(CoreExpr::If {
                cond: Box::new(CoreExpr::Gt(
                    Box::new(CoreExpr::Var("total".to_string())),
                    Box::new(CoreExpr::Literal(LiteralValue::Int(10)))
                )),
                then_: Box::new(CoreExpr::Var("total".to_string())),
                else_: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
            }),
        }
    );
}

#[test]
fn rejects_non_identifier_let_binding() {
    let err = parse_expr("let(add(x, y), 1, 2)").unwrap_err();
    assert_eq!(err.message, "let binding name must be an identifier");
}

#[test]
fn parses_short_circuit_boolean_forms() {
    assert_eq!(
        parse_expr("and(flag, gt(total, 0))").unwrap(),
        CoreExpr::And {
            left: Box::new(CoreExpr::Var("flag".to_string())),
            right: Box::new(CoreExpr::Gt(
                Box::new(CoreExpr::Var("total".to_string())),
                Box::new(CoreExpr::Literal(LiteralValue::Int(0)))
            )),
        }
    );
    assert_eq!(
        parse_expr("or(flag, eq(total, 0))").unwrap(),
        CoreExpr::Or {
            left: Box::new(CoreExpr::Var("flag".to_string())),
            right: Box::new(CoreExpr::Eq(
                Box::new(CoreExpr::Var("total".to_string())),
                Box::new(CoreExpr::Literal(LiteralValue::Int(0)))
            )),
        }
    );
}

#[test]
fn parses_match_expression() {
    assert_eq!(
        parse_expr("match(score, 1, 10, 2, 20, _, 0)").unwrap(),
        CoreExpr::Match {
            scrutinee: Box::new(CoreExpr::Var("score".to_string())),
            arms: vec![
                MatchArm {
                    pattern: "1".to_string(),
                    body: CoreExpr::Literal(LiteralValue::Int(10)),
                },
                MatchArm {
                    pattern: "2".to_string(),
                    body: CoreExpr::Literal(LiteralValue::Int(20)),
                },
                MatchArm {
                    pattern: "_".to_string(),
                    body: CoreExpr::Literal(LiteralValue::Int(0)),
                },
            ],
        }
    );
}

#[test]
fn parses_match_constructor_pattern() {
    assert_eq!(
        parse_expr("match(result, Ok(value), value, _, 0)").unwrap(),
        CoreExpr::Match {
            scrutinee: Box::new(CoreExpr::Var("result".to_string())),
            arms: vec![
                MatchArm {
                    pattern: "Ok(value)".to_string(),
                    body: CoreExpr::Var("value".to_string()),
                },
                MatchArm {
                    pattern: "_".to_string(),
                    body: CoreExpr::Literal(LiteralValue::Int(0)),
                },
            ],
        }
    );
}

#[test]
fn parses_compound_value_forms() {
    assert_eq!(
        parse_expr("record(age, 30, score, add(10, 5))").unwrap(),
        CoreExpr::RecordNew {
            fields: vec![
                ("age".to_string(), CoreExpr::Literal(LiteralValue::Int(30))),
                (
                    "score".to_string(),
                    CoreExpr::Add(
                        Box::new(CoreExpr::Literal(LiteralValue::Int(10))),
                        Box::new(CoreExpr::Literal(LiteralValue::Int(5))),
                    ),
                ),
            ],
        }
    );

    assert_eq!(
        parse_expr("field(person, age)").unwrap(),
        CoreExpr::FieldGet {
            record: Box::new(CoreExpr::Var("person".to_string())),
            field: "age".to_string(),
        }
    );

    assert_eq!(
        parse_expr("variant(Some, 7)").unwrap(),
        CoreExpr::VariantNew {
            tag: "Some".to_string(),
            payload: Some(Box::new(CoreExpr::Literal(LiteralValue::Int(7)))),
        }
    );

    assert_eq!(
        parse_expr("list(1, 2, 3)").unwrap(),
        CoreExpr::ListNew(vec![
            CoreExpr::Literal(LiteralValue::Int(1)),
            CoreExpr::Literal(LiteralValue::Int(2)),
            CoreExpr::Literal(LiteralValue::Int(3)),
        ])
    );
}

#[test]
fn rejects_malformed_compound_value_forms() {
    let err = parse_expr("record(age, 30, dangling)").unwrap_err();
    assert_eq!(err.message, "record expects field/value pairs, got 3 args");

    let err = parse_expr("field(person, 1)").unwrap_err();
    assert_eq!(err.message, "field name must be an identifier");

    let err = parse_expr("variant(Some, 1, 2)").unwrap_err();
    assert_eq!(err.message, "variant expects 1 or 2 args, got 3");
}

#[test]
fn rejects_match_without_pattern_body_pairs() {
    let err = parse_expr("match(value, 1)").unwrap_err();
    assert_eq!(
        err.message,
        "match expects scrutinee plus pattern/body pairs, got 2 args"
    );
}

// ── New comparison and boolean operators ─────────────────────────────

#[test]
fn parses_ne_le_ge_comparison_operators() {
    assert_eq!(
        parse_expr("ne(x, y)").unwrap(),
        CoreExpr::Ne(
            Box::new(CoreExpr::Var("x".to_string())),
            Box::new(CoreExpr::Var("y".to_string()))
        )
    );
    assert_eq!(
        parse_expr("le(x, 10)").unwrap(),
        CoreExpr::Le(
            Box::new(CoreExpr::Var("x".to_string())),
            Box::new(CoreExpr::Literal(LiteralValue::Int(10)))
        )
    );
    assert_eq!(
        parse_expr("ge(score, 0)").unwrap(),
        CoreExpr::Ge(
            Box::new(CoreExpr::Var("score".to_string())),
            Box::new(CoreExpr::Literal(LiteralValue::Int(0)))
        )
    );
}

#[test]
fn parses_not_operator() {
    assert_eq!(
        parse_expr("not(flag)").unwrap(),
        CoreExpr::Not(Box::new(CoreExpr::Var("flag".to_string())))
    );
    // not applied to a comparison
    assert_eq!(
        parse_expr("not(eq(x, 0))").unwrap(),
        CoreExpr::Not(Box::new(CoreExpr::Eq(
            Box::new(CoreExpr::Var("x".to_string())),
            Box::new(CoreExpr::Literal(LiteralValue::Int(0)))
        )))
    );
}

// ── Float and string literals ────────────────────────────────────────

#[test]
#[allow(clippy::approx_constant)] // 3.14 is intentional parser test data, not a PI approximation
fn parses_float_literals() {
    match parse_expr("3.14").unwrap() {
        CoreExpr::Literal(LiteralValue::Float(f)) => {
            assert!((f - 3.14).abs() < 1e-10, "expected 3.14, got {f}");
        }
        other => panic!("expected Float literal, got {other:?}"),
    }
    match parse_expr("-2.5").unwrap() {
        CoreExpr::Literal(LiteralValue::Float(f)) => {
            assert!((f - (-2.5)).abs() < 1e-10, "expected -2.5, got {f}");
        }
        other => panic!("expected Float literal, got {other:?}"),
    }
}

#[test]
fn parses_string_literals() {
    assert_eq!(
        parse_expr("\"hello\"").unwrap(),
        CoreExpr::Literal(LiteralValue::Text("hello".to_string()))
    );
    assert_eq!(
        parse_expr("\"hello world\"").unwrap(),
        CoreExpr::Literal(LiteralValue::Text("hello world".to_string()))
    );
    // Escape sequences
    assert_eq!(
        parse_expr("\"say \\\"hi\\\"\"").unwrap(),
        CoreExpr::Literal(LiteralValue::Text("say \"hi\"".to_string()))
    );
    assert_eq!(
        parse_expr("\"line\\nnewline\"").unwrap(),
        CoreExpr::Literal(LiteralValue::Text("line\nnewline".to_string()))
    );
}

#[test]
fn rejects_unterminated_string_literal() {
    let err = parse_expr("\"unterminated").unwrap_err();
    assert_eq!(err.message, "unterminated string literal");
}

// ── Option/Result convenience constructors ───────────────────────────

#[test]
fn parses_option_result_convenience_constructors() {
    assert_eq!(
        parse_expr("none()").unwrap(),
        CoreExpr::VariantNew {
            tag: "None".to_string(),
            payload: None,
        }
    );
    assert_eq!(
        parse_expr("some(42)").unwrap(),
        CoreExpr::VariantNew {
            tag: "Some".to_string(),
            payload: Some(Box::new(CoreExpr::Literal(LiteralValue::Int(42)))),
        }
    );
    assert_eq!(
        parse_expr("ok(x)").unwrap(),
        CoreExpr::VariantNew {
            tag: "Ok".to_string(),
            payload: Some(Box::new(CoreExpr::Var("x".to_string()))),
        }
    );
    assert_eq!(
        parse_expr("err(msg)").unwrap(),
        CoreExpr::VariantNew {
            tag: "Err".to_string(),
            payload: Some(Box::new(CoreExpr::Var("msg".to_string()))),
        }
    );
}

#[test]
fn rejects_none_with_arguments() {
    let err = parse_expr("none(x)").unwrap_err();
    assert_eq!(err.message, "none expects 0 args, got 1");
}

// ── Match with Option/Result constructor patterns ─────────────────────

#[test]
fn parses_match_with_option_constructor_patterns() {
    // match(opt, Some(v), v, None, 0)
    assert_eq!(
        parse_expr("match(opt, Some(v), v, None, 0)").unwrap(),
        CoreExpr::Match {
            scrutinee: Box::new(CoreExpr::Var("opt".to_string())),
            arms: vec![
                MatchArm {
                    pattern: "Some(v)".to_string(),
                    body: CoreExpr::Var("v".to_string()),
                },
                MatchArm {
                    pattern: "None".to_string(),
                    body: CoreExpr::Literal(LiteralValue::Int(0)),
                },
            ],
        }
    );
}

#[test]
fn parses_match_with_result_constructor_patterns() {
    // match(result, Ok(val), val, Err(e), -1)
    assert_eq!(
        parse_expr("match(result, Ok(val), val, Err(e), -1)").unwrap(),
        CoreExpr::Match {
            scrutinee: Box::new(CoreExpr::Var("result".to_string())),
            arms: vec![
                MatchArm {
                    pattern: "Ok(val)".to_string(),
                    body: CoreExpr::Var("val".to_string()),
                },
                MatchArm {
                    pattern: "Err(e)".to_string(),
                    body: CoreExpr::Literal(LiteralValue::Int(-1)),
                },
            ],
        }
    );
}

// ── Control flow forms ───────────────────────────────────────────────

#[test]
fn parses_loop_break_continue() {
    assert_eq!(
        parse_expr("loop(break(42))").unwrap(),
        CoreExpr::Loop {
            body: Box::new(CoreExpr::Break {
                value: Box::new(CoreExpr::Literal(LiteralValue::Int(42))),
            }),
            termination: None,
        }
    );
    assert_eq!(
        parse_expr("loop(continue())").unwrap(),
        CoreExpr::Loop {
            body: Box::new(CoreExpr::Continue),
            termination: None,
        }
    );
}

#[test]
fn parses_while_loop() {
    assert_eq!(
        parse_expr("while(flag, break(0))").unwrap(),
        CoreExpr::WhileLoop {
            cond: Box::new(CoreExpr::Var("flag".to_string())),
            body: Box::new(CoreExpr::Break {
                value: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
            }),
            termination: None,
        }
    );
}

#[test]
fn parses_return_expression() {
    assert_eq!(
        parse_expr("return(x)").unwrap(),
        CoreExpr::Return {
            value: Box::new(CoreExpr::Var("x".to_string())),
        }
    );
}

#[test]
fn rejects_continue_with_arguments() {
    let err = parse_expr("continue(1)").unwrap_err();
    assert_eq!(err.message, "continue expects 0 args, got 1");
}

// ── Effect and lambda forms ──────────────────────────────────────────

#[test]
fn parses_print_as_log_write_effect_call() {
    assert_eq!(
        parse_expr("print(\"Hello, world!\")").unwrap(),
        CoreExpr::EffectCall {
            capability: "log.write".to_string(),
            func: "write".to_string(),
            args: vec![CoreExpr::Literal(LiteralValue::Text(
                "Hello, world!".to_string()
            ))],
        }
    );
}

#[test]
fn rejects_print_non_text_literal() {
    let err = parse_expr("print(42)").unwrap_err();
    assert_eq!(err.message, "print expects a Text literal argument");
}

#[test]
fn parses_effect_call() {
    assert_eq!(
        parse_expr("effect_call(database.read, Cart, cartId)").unwrap(),
        CoreExpr::EffectCall {
            capability: "database.read".to_string(),
            func: "Cart".to_string(),
            args: vec![CoreExpr::Var("cartId".to_string())],
        }
    );
    // No-arg effect call
    assert_eq!(
        parse_expr("effect_call(clock, now)").unwrap(),
        CoreExpr::EffectCall {
            capability: "clock".to_string(),
            func: "now".to_string(),
            args: vec![],
        }
    );
}

#[test]
fn rejects_effect_call_without_enough_args() {
    let err = parse_expr("effect_call(clock)").unwrap_err();
    assert!(
        err.message.contains("at least 2 args"),
        "expected 'at least 2 args' error, got: {}",
        err.message
    );
}

#[test]
fn parses_lambda_expressions() {
    // Zero-param lambda
    assert_eq!(
        parse_expr("lambda(42)").unwrap(),
        CoreExpr::Lambda {
            params: vec![],
            body: Box::new(CoreExpr::Literal(LiteralValue::Int(42))),
        }
    );
    // Single-param lambda
    assert_eq!(
        parse_expr("lambda(x, add(x, 1))").unwrap(),
        CoreExpr::Lambda {
            params: vec!["x".to_string()],
            body: Box::new(CoreExpr::Add(
                Box::new(CoreExpr::Var("x".to_string())),
                Box::new(CoreExpr::Literal(LiteralValue::Int(1)))
            )),
        }
    );
    // Multi-param lambda
    assert_eq!(
        parse_expr("lambda(x, y, add(x, y))").unwrap(),
        CoreExpr::Lambda {
            params: vec!["x".to_string(), "y".to_string()],
            body: Box::new(CoreExpr::Add(
                Box::new(CoreExpr::Var("x".to_string())),
                Box::new(CoreExpr::Var("y".to_string()))
            )),
        }
    );
}

#[test]
fn rejects_lambda_with_no_arguments() {
    let err = parse_expr("lambda()").unwrap_err();
    assert!(
        err.message.contains("at least 1 arg"),
        "expected 'at least 1 arg' error, got: {}",
        err.message
    );
}

// ── Collection and cell forms ────────────────────────────────────────

#[test]
fn parses_foreach_and_fold() {
    assert_eq!(
        parse_expr("foreach(item, items, add(acc, item))").unwrap(),
        CoreExpr::ForEach {
            binding: "item".to_string(),
            collection: Box::new(CoreExpr::Var("items".to_string())),
            body: Box::new(CoreExpr::Add(
                Box::new(CoreExpr::Var("acc".to_string())),
                Box::new(CoreExpr::Var("item".to_string()))
            )),
        }
    );
    assert_eq!(
        parse_expr("fold(0, items, add_item)").unwrap(),
        CoreExpr::Fold {
            init: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
            list: Box::new(CoreExpr::Var("items".to_string())),
            func: Box::new(CoreExpr::Var("add_item".to_string())),
        }
    );
}

#[test]
fn parses_cell_operations() {
    assert_eq!(
        parse_expr("cell_new(0)").unwrap(),
        CoreExpr::CellNew {
            init: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
        }
    );
    assert_eq!(
        parse_expr("cell_get(counter)").unwrap(),
        CoreExpr::CellGet {
            cell: Box::new(CoreExpr::Var("counter".to_string())),
        }
    );
    assert_eq!(
        parse_expr("cell_set(counter, add(cell_get(counter), 1))").unwrap(),
        CoreExpr::CellSet {
            cell: Box::new(CoreExpr::Var("counter".to_string())),
            value: Box::new(CoreExpr::Add(
                Box::new(CoreExpr::CellGet {
                    cell: Box::new(CoreExpr::Var("counter".to_string()))
                }),
                Box::new(CoreExpr::Literal(LiteralValue::Int(1)))
            )),
        }
    );
}

// ── Map and Set constructor forms ────────────────────────────────────

#[test]
fn parses_map_empty() {
    assert_eq!(
        parse_expr("map()").unwrap(),
        CoreExpr::MapNew { entries: vec![] }
    );
}

#[test]
fn parses_map_single_pair() {
    assert_eq!(
        parse_expr("map(1, 10)").unwrap(),
        CoreExpr::MapNew {
            entries: vec![(
                CoreExpr::Literal(LiteralValue::Int(1)),
                CoreExpr::Literal(LiteralValue::Int(10)),
            )],
        }
    );
}

#[test]
fn parses_map_two_pairs() {
    assert_eq!(
        parse_expr("map(1, 10, 2, 20)").unwrap(),
        CoreExpr::MapNew {
            entries: vec![
                (
                    CoreExpr::Literal(LiteralValue::Int(1)),
                    CoreExpr::Literal(LiteralValue::Int(10)),
                ),
                (
                    CoreExpr::Literal(LiteralValue::Int(2)),
                    CoreExpr::Literal(LiteralValue::Int(20)),
                ),
            ],
        }
    );
}

#[test]
fn parses_map_with_expression_values() {
    // map(x, add(x, 1)) — key is a Var, value is an Add expression
    assert_eq!(
        parse_expr("map(x, add(x, 1))").unwrap(),
        CoreExpr::MapNew {
            entries: vec![(
                CoreExpr::Var("x".to_string()),
                CoreExpr::Add(
                    Box::new(CoreExpr::Var("x".to_string())),
                    Box::new(CoreExpr::Literal(LiteralValue::Int(1))),
                ),
            )],
        }
    );
}

#[test]
fn rejects_map_with_odd_arity() {
    let err = parse_expr("map(k1)").unwrap_err();
    assert_eq!(
        err.message,
        "map expects an even number of args (key/value pairs), got 1"
    );
}

#[test]
fn rejects_map_with_odd_arity_three() {
    let err = parse_expr("map(k1, v1, k2)").unwrap_err();
    assert_eq!(
        err.message,
        "map expects an even number of args (key/value pairs), got 3"
    );
}

#[test]
fn parses_set_empty() {
    assert_eq!(
        parse_expr("set()").unwrap(),
        CoreExpr::SetNew { elements: vec![] }
    );
}

#[test]
fn parses_set_single_element() {
    assert_eq!(
        parse_expr("set(42)").unwrap(),
        CoreExpr::SetNew {
            elements: vec![CoreExpr::Literal(LiteralValue::Int(42))],
        }
    );
}

#[test]
fn parses_set_multiple_elements() {
    assert_eq!(
        parse_expr("set(1, 2, 3)").unwrap(),
        CoreExpr::SetNew {
            elements: vec![
                CoreExpr::Literal(LiteralValue::Int(1)),
                CoreExpr::Literal(LiteralValue::Int(2)),
                CoreExpr::Literal(LiteralValue::Int(3)),
            ],
        }
    );
}

#[test]
fn parses_set_with_expression_elements() {
    // set(add(x, 1), mul(y, 2)) — elements are arbitrary expressions
    assert_eq!(
        parse_expr("set(add(x, 1), mul(y, 2))").unwrap(),
        CoreExpr::SetNew {
            elements: vec![
                CoreExpr::Add(
                    Box::new(CoreExpr::Var("x".to_string())),
                    Box::new(CoreExpr::Literal(LiteralValue::Int(1))),
                ),
                CoreExpr::Mul(
                    Box::new(CoreExpr::Var("y".to_string())),
                    Box::new(CoreExpr::Literal(LiteralValue::Int(2))),
                ),
            ],
        }
    );
}

// ── IndexGet constructor form ─────────────────────────────────────────

#[test]
fn parses_index_valid_form() {
    // index(lst, 0) — collection is a Var, index is an integer literal.
    assert_eq!(
        parse_expr("index(lst, 0)").unwrap(),
        CoreExpr::IndexGet {
            collection: Box::new(CoreExpr::Var("lst".to_string())),
            index: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
        }
    );
}

#[test]
fn parses_index_with_expression_index() {
    // index(lst, add(i, 1)) — index argument is an arbitrary expression.
    assert_eq!(
        parse_expr("index(lst, add(i, 1))").unwrap(),
        CoreExpr::IndexGet {
            collection: Box::new(CoreExpr::Var("lst".to_string())),
            index: Box::new(CoreExpr::Add(
                Box::new(CoreExpr::Var("i".to_string())),
                Box::new(CoreExpr::Literal(LiteralValue::Int(1))),
            )),
        }
    );
}

#[test]
fn rejects_index_wrong_arity_one() {
    let err = parse_expr("index(x)").unwrap_err();
    assert_eq!(err.message, "index expects 2 args, got 1");
}

#[test]
fn rejects_index_wrong_arity_three() {
    let err = parse_expr("index(x, 0, y)").unwrap_err();
    assert_eq!(err.message, "index expects 2 args, got 3");
}

// ── Nested expressions with new operators ────────────────────────────

#[test]
fn parses_nested_range_check_with_le_ge() {
    // and(ge(x, 0), le(x, 100))  — checks 0 <= x <= 100
    assert_eq!(
        parse_expr("and(ge(x, 0), le(x, 100))").unwrap(),
        CoreExpr::And {
            left: Box::new(CoreExpr::Ge(
                Box::new(CoreExpr::Var("x".to_string())),
                Box::new(CoreExpr::Literal(LiteralValue::Int(0)))
            )),
            right: Box::new(CoreExpr::Le(
                Box::new(CoreExpr::Var("x".to_string())),
                Box::new(CoreExpr::Literal(LiteralValue::Int(100)))
            )),
        }
    );
}

// ── abort() form ──────────────────────────────────────────────────────

// PARSE-ABORT-1: abort("message") lowers to CoreExpr::Abort with the
// string literal as the message.
#[test]
fn parses_abort_with_string_literal() {
    assert_eq!(
        parse_expr("abort(\"unreachable branch\")").unwrap(),
        CoreExpr::Abort {
            message: "unreachable branch".to_string(),
        }
    );
}

// PARSE-ABORT-2: abort(add(x, y)) — a non-literal, non-identifier argument
// is a parse error; the message must be a string literal or bare identifier.
#[test]
fn rejects_abort_with_non_literal_expression() {
    let err = parse_expr("abort(add(x, y))").unwrap_err();
    assert_eq!(
        err.message,
        "abort expects a string literal or identifier as the message"
    );
}

// PARSE-ABORT-3: abort() with wrong arity (zero args) is rejected.
#[test]
fn rejects_abort_with_zero_args() {
    let err = parse_expr("abort()").unwrap_err();
    assert!(
        err.message.contains("abort"),
        "expected arity error mentioning 'abort', got: {}",
        err.message
    );
}
