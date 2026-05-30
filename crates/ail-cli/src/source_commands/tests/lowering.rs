use super::*;

#[test]
fn lowers_source_to_acl_create_ops() {
    let program =
        parse_ail_source("fn main() -> Int = add(20, 22)\ntest add = eq(add(20, 22), 42)")
            .expect("source must parse");
    let acl = source_program_to_acl(&program, "source_main".to_string());

    assert!(acl.contains("op create_function id=fn.main return=Int body=add(20, 22)"));
    assert!(acl.contains("op create_test id=test.add return=Bool body=eq(add(20, 22), 42)"));
}

#[test]
fn lowers_source_consts_to_zero_arg_functions() {
    let program = parse_ail_source(
        "const answer: Int = 40 + 2\nfn main() -> Int = answer\ntest answer = answer == 42",
    )
    .expect("source must parse");
    let acl = source_program_to_acl(&program, "source_const".to_string());

    assert_eq!(program.constants[0].name, "fn.answer");
    assert!(acl.contains("op create_function id=fn.answer return=Int body=add(40, 2)"));
    assert!(acl.contains("op create_function id=fn.main return=Int body=answer()"));
    assert!(acl.contains("op create_test id=test.answer return=Bool body=eq(answer(), 42)"));
}

#[test]
fn lowers_source_set_and_map_collections() {
    let program = parse_ail_source(
        r#"
fn ids() -> Set<Int> = set(1, 2 + 3)
fn labels() -> Map<Text, Int> = map("one", 1, "two", 2)
"#,
    )
    .expect("source set/map must parse");
    let acl = source_program_to_acl(&program, "source_collections".to_string());

    assert_eq!(program.functions[0].return_type, "Set<Int>");
    assert_eq!(program.functions[1].return_type, "Map<Text,Int>");
    assert!(acl.contains("op create_function id=fn.ids return=Set<Int> body=set(1, add(2, 3))"));
    assert!(acl.contains(
        r#"op create_function id=fn.labels return=Map<Text,Int> body=map("one", 1, "two", 2)"#
    ));
}

#[test]
fn lowers_source_tuple_collections() {
    let program = parse_ail_source(
        r#"
fn pair() -> Tuple<Int, Text> = tuple(42, "answer")
"#,
    )
    .expect("source tuple must parse");
    let acl = source_program_to_acl(&program, "source_tuple".to_string());

    assert_eq!(program.functions[0].return_type, "Tuple<Int,Text>");
    assert!(acl.contains(
        r#"op create_function id=fn.pair return=Tuple<Int,Text> body=tuple(42, "answer")"#
    ));
}

#[test]
fn lowers_source_record_field_access_and_update() {
    let program = parse_ail_source(
        r#"
fn person() -> Record<age: Int, name: Text> = { age: 42, name: "Ada" }
fn age() -> Int = field(person(), age)
fn age_dot() -> Int = person().age
fn older() -> Record<age: Int, name: Text> = { ...person(), age: 43 }
fn older_renamed() -> Record<age: Int, name: Text> = { ...person(), age: 43, name: "Grace" }
"#,
    )
    .expect("source record must parse");
    let acl = source_program_to_acl(&program, "source_record".to_string());

    assert_eq!(
        program.functions[0].return_type,
        "Record<age:Int,name:Text>"
    );
    assert!(acl.contains(
            r#"op create_function id=fn.person return=Record<age:Int,name:Text> body=record(age, 42, name, "Ada")"#
        ));
    assert!(acl.contains("op create_function id=fn.age return=Int body=field(person(), age)"));
    assert!(acl.contains("op create_function id=fn.age_dot return=Int body=field(person(), age)"));
    assert!(
            acl.contains(
                "op create_function id=fn.older return=Record<age:Int,name:Text> body=update(person(), age, 43)"
            )
        );
    assert!(acl.contains(
            r#"op create_function id=fn.older_renamed return=Record<age:Int,name:Text> body=update(update(person(), age, 43), name, "Grace")"#
        ));
}

#[test]
fn lowers_source_option_result_constructors() {
    let program = parse_ail_source(
        r#"
fn maybe(flag: Bool) -> Option<Int> = if flag { Some(42) } else { None }
fn ok_value() -> Result<Int, Text> = Ok(42)
fn err_value() -> Result<Int, Text> = Err("boom")
"#,
    )
    .expect("source constructors must parse");
    let acl = source_program_to_acl(&program, "source_constructors".to_string());

    assert!(acl.contains(
        "op create_function id=fn.maybe return=Option<Int> body=if(flag, some(42), none())"
    ));
    assert!(acl.contains("op create_function id=fn.ok_value return=Result<Int,Text> body=ok(42)"));
    assert!(acl.contains(
        r#"op create_function id=fn.err_value return=Result<Int,Text> body=err("boom")"#
    ));
}

#[test]
fn lowers_source_unwrap_or_helper() {
    let program = parse_ail_source(
        r#"
fn value(input: Option<Int>) -> Int = unwrap_or(input, 0)
"#,
    )
    .expect("source unwrap_or must parse");
    let acl = source_program_to_acl(&program, "source_unwrap_or".to_string());

    assert!(acl.contains(
            "op create_function id=fn.value return=Int body=match(input, Some(__ail_unwrap), __ail_unwrap, None, 0)"
        ));
}

#[test]
fn lowers_source_option_predicate_helpers() {
    let program = parse_ail_source(
        r#"
fn has_value(input: Option<Int>) -> Bool = is_some(input)
fn missing(input: Option<Int>) -> Bool = is_none(input)
"#,
    )
    .expect("source option predicates must parse");
    let acl = source_program_to_acl(&program, "source_option_predicates".to_string());

    assert!(acl.contains(
            "op create_function id=fn.has_value return=Bool body=match(input, Some(_), true, None, false)"
        ));
    assert!(acl.contains(
        "op create_function id=fn.missing return=Bool body=match(input, Some(_), false, None, true)"
    ));
}

#[test]
fn lowers_source_result_predicate_helpers() {
    let program = parse_ail_source(
        r#"
fn succeeded(input: Result<Int, Text>) -> Bool = is_ok(input)
fn failed(input: Result<Int, Text>) -> Bool = is_err(input)
"#,
    )
    .expect("source result predicates must parse");
    let acl = source_program_to_acl(&program, "source_result_predicates".to_string());

    assert!(acl.contains(
            "op create_function id=fn.succeeded return=Bool body=match(input, Ok(_), true, Err(_), false)"
        ));
    assert!(acl.contains(
        "op create_function id=fn.failed return=Bool body=match(input, Ok(_), false, Err(_), true)"
    ));
}

#[test]
fn lowers_source_first_or_helper() {
    let program = parse_ail_source(
        r#"
fn first(values: List<Int>) -> Int = first_or(values, 0)
"#,
    )
    .expect("source first_or must parse");
    let acl = source_program_to_acl(&program, "source_first_or".to_string());

    assert!(acl.contains(
        "op create_function id=fn.first return=Int body=if(gt(len(values), 0), index(values, 0), 0)"
    ));
}

#[test]
fn lowers_source_last_or_helper() {
    let program = parse_ail_source(
        r#"
fn last(values: List<Int>) -> Int = last_or(values, 0)
"#,
    )
    .expect("source last_or must parse");
    let acl = source_program_to_acl(&program, "source_last_or".to_string());

    assert!(acl.contains(
            "op create_function id=fn.last return=Int body=if(gt(len(values), 0), index(values, sub(len(values), 1)), 0)"
        ));
}

#[test]
fn lowers_source_get_or_helper() {
    let program = parse_ail_source(
        r#"
fn item(values: List<Int>, idx: Int) -> Int = get_or(values, idx, 0)
"#,
    )
    .expect("source get_or must parse");
    let acl = source_program_to_acl(&program, "source_get_or".to_string());

    assert!(acl.contains(
            "op create_function id=fn.item return=Int body=if(and(ge(idx, 0), lt(idx, len(values))), index(values, idx), 0)"
        ));
}

#[test]
fn lowers_source_is_empty_helper() {
    let program = parse_ail_source(
        r#"
fn no_items(values: List<Int>) -> Bool = is_empty(values)
fn no_text(value: Text) -> Bool = is_empty(value)
"#,
    )
    .expect("source is_empty must parse");
    let acl = source_program_to_acl(&program, "source_is_empty".to_string());

    assert!(acl.contains("op create_function id=fn.no_items return=Bool body=eq(len(values), 0)"));
    assert!(acl.contains("op create_function id=fn.no_text return=Bool body=eq(len(value), 0)"));
}

#[test]
fn lowers_source_infix_arithmetic_with_precedence() {
    let program =
        parse_ail_source("test math = 10 - 2 * 3 + 8 / 4 + 7 % 4 == 9").expect("source must parse");
    let acl = source_program_to_acl(&program, "source_math".to_string());

    assert!(acl.contains(
            "op create_test id=test.math return=Bool body=eq(add(add(sub(10, mul(2, 3)), div(8, 4)), mod(7, 4)), 9)"
        ));
}

#[test]
fn lowers_source_text_concat_operator() {
    let program = parse_ail_source(
        r#"
fn greeting(name: Text) -> Text = "Hello, " ++ name
test greeting = "Hello, " ++ "AIL" == "Hello, AIL"
"#,
    )
    .expect("source text concat operator must parse");
    let acl = source_program_to_acl(&program, "source_text_concat_operator".to_string());

    assert!(
        acl.contains(
            r#"op create_function id=fn.greeting return=Text body=concat("Hello, ", name)"#
        )
    );
    assert!(acl.contains(
            r#"op create_test id=test.greeting return=Bool body=eq(concat("Hello, ", "AIL"), "Hello, AIL")"#
        ));
}

#[test]
fn lowers_source_text_eq_helper() {
    let program = parse_ail_source(
        r#"
fn same(left: Text, right: Text) -> Bool = text_eq(left, right)
"#,
    )
    .expect("source text_eq must parse");
    let acl = source_program_to_acl(&program, "source_text_eq".to_string());

    assert!(acl.contains("op create_function id=fn.same return=Bool body=text.eq(left, right)"));
}

#[test]
fn lowers_source_int_bounds_helpers() {
    let program = parse_ail_source(
        r#"
fn low(left: Int, right: Int) -> Int = int_min(left, right)
fn high(left: Int, right: Int) -> Int = int_max(left, right)
fn bounded(value: Int, low: Int, high: Int) -> Int = int_clamp(value, low, high)
fn magnitude(value: Int, fallback: Int) -> Int = int_abs_or(value, fallback)
fn negated(value: Int, fallback: Int) -> Int = int_neg_or(value, fallback)
fn summed(left: Int, right: Int, fallback: Int) -> Int = int_add_or(left, right, fallback)
fn difference(left: Int, right: Int, fallback: Int) -> Int = int_sub_or(left, right, fallback)
fn product(left: Int, right: Int, fallback: Int) -> Int = int_mul_or(left, right, fallback)
fn saturated(left: Int, right: Int) -> Int = int_saturating_add(left, right)
fn saturated_difference(left: Int, right: Int) -> Int = int_saturating_sub(left, right)
fn saturated_product(left: Int, right: Int) -> Int = int_saturating_mul(left, right)
fn saturated_negated(value: Int) -> Int = int_saturating_neg(value)
fn wrapped_sum(left: Int, right: Int) -> Int = int_wrapping_add(left, right)
fn wrapped_difference(left: Int, right: Int) -> Int = int_wrapping_sub(left, right)
fn wrapped_product(left: Int, right: Int) -> Int = int_wrapping_mul(left, right)
fn wrapped_negated(value: Int) -> Int = int_wrapping_neg(value)
fn masked(left: Int, right: Int) -> Int = int_bit_and(left, right)
fn flagged(left: Int, right: Int) -> Int = int_bit_or(left, right)
fn toggled(left: Int, right: Int) -> Int = int_bit_xor(left, right)
fn inverted(value: Int) -> Int = int_bit_not(value)
fn shifted_left(value: Int, amount: Int) -> Int = int_shift_left(value, amount)
fn shifted_right(value: Int, amount: Int) -> Int = int_shift_right(value, amount)
fn shifted_right_unsigned(value: Int, amount: Int) -> Int = int_shift_right_unsigned(value, amount)
fn quotient(value: Int, divisor: Int, fallback: Int) -> Int = int_div_or(value, divisor, fallback)
fn remainder(value: Int, divisor: Int, fallback: Int) -> Int = int_rem_or(value, divisor, fallback)
"#,
    )
    .expect("source int bounds helpers must parse");
    let acl = source_program_to_acl(&program, "source_int_bounds".to_string());

    assert!(acl.contains("op create_function id=fn.low return=Int body=int.min(left, right)"));
    assert!(acl.contains("op create_function id=fn.high return=Int body=int.max(left, right)"));
    assert!(
        acl.contains(
            "op create_function id=fn.bounded return=Int body=int.clamp(value, low, high)"
        )
    );
    assert!(acl.contains(
        "op create_function id=fn.magnitude return=Int body=int.abs_or(value, fallback)"
    ));
    assert!(
        acl.contains(
            "op create_function id=fn.negated return=Int body=int.neg_or(value, fallback)"
        )
    );
    assert!(acl.contains(
        "op create_function id=fn.summed return=Int body=int.add_or(left, right, fallback)"
    ));
    assert!(acl.contains(
        "op create_function id=fn.difference return=Int body=int.sub_or(left, right, fallback)"
    ));
    assert!(acl.contains(
        "op create_function id=fn.product return=Int body=int.mul_or(left, right, fallback)"
    ));
    assert!(acl.contains(
        "op create_function id=fn.saturated return=Int body=int.saturating_add(left, right)"
    ));
    assert!(acl.contains(
            "op create_function id=fn.saturated_difference return=Int body=int.saturating_sub(left, right)"
        ));
    assert!(acl.contains(
        "op create_function id=fn.saturated_product return=Int body=int.saturating_mul(left, right)"
    ));
    assert!(acl.contains(
        "op create_function id=fn.saturated_negated return=Int body=int.saturating_neg(value)"
    ));
    assert!(acl.contains(
        "op create_function id=fn.wrapped_sum return=Int body=int.wrapping_add(left, right)"
    ));
    assert!(acl.contains(
        "op create_function id=fn.wrapped_difference return=Int body=int.wrapping_sub(left, right)"
    ));
    assert!(acl.contains(
        "op create_function id=fn.wrapped_product return=Int body=int.wrapping_mul(left, right)"
    ));
    assert!(acl.contains(
        "op create_function id=fn.wrapped_negated return=Int body=int.wrapping_neg(value)"
    ));
    assert!(
        acl.contains("op create_function id=fn.masked return=Int body=int.bit_and(left, right)")
    );
    assert!(
        acl.contains("op create_function id=fn.flagged return=Int body=int.bit_or(left, right)")
    );
    assert!(
        acl.contains("op create_function id=fn.toggled return=Int body=int.bit_xor(left, right)")
    );
    assert!(acl.contains("op create_function id=fn.inverted return=Int body=int.bit_not(value)"));
    assert!(acl.contains(
        "op create_function id=fn.shifted_left return=Int body=int.shift_left(value, amount)"
    ));
    assert!(acl.contains(
        "op create_function id=fn.shifted_right return=Int body=int.shift_right(value, amount)"
    ));
    assert!(acl.contains(
            "op create_function id=fn.shifted_right_unsigned return=Int body=int.shift_right_unsigned(value, amount)"
        ));
    assert!(acl.contains(
        "op create_function id=fn.quotient return=Int body=int.div_or(value, divisor, fallback)"
    ));
    assert!(acl.contains(
        "op create_function id=fn.remainder return=Int body=int.rem_or(value, divisor, fallback)"
    ));
}

#[test]
fn lowers_source_text_trim_helper() {
    let program = parse_ail_source(
        r#"
fn cleaned(value: Text) -> Text = text_trim(value)
"#,
    )
    .expect("source text_trim must parse");
    let acl = source_program_to_acl(&program, "source_text_trim".to_string());

    assert!(acl.contains("op create_function id=fn.cleaned return=Text body=text.trim(value)"));
}

#[test]
fn lowers_source_text_contains_helper() {
    let program = parse_ail_source(
        r#"
fn has(haystack: Text, needle: Text) -> Bool = text_contains(haystack, needle)
"#,
    )
    .expect("source text_contains must parse");
    let acl = source_program_to_acl(&program, "source_text_contains".to_string());

    assert!(
        acl.contains(
            "op create_function id=fn.has return=Bool body=text.contains(haystack, needle)"
        )
    );
}

#[test]
fn lowers_source_text_index_of_helper() {
    let program = parse_ail_source(
        r#"
fn find(haystack: Text, needle: Text) -> Int = text_index_of(haystack, needle)
"#,
    )
    .expect("source text_index_of must parse");
    let acl = source_program_to_acl(&program, "source_text_index_of".to_string());

    assert!(
        acl.contains(
            "op create_function id=fn.find return=Int body=text.index_of(haystack, needle)"
        )
    );
}

#[test]
fn lowers_source_text_byte_at_or_helper() {
    let program = parse_ail_source(
        r#"
fn byte(value: Text, index: Int, fallback: Int) -> Int = text_byte_at_or(value, index, fallback)
"#,
    )
    .expect("source text_byte_at_or must parse");
    let acl = source_program_to_acl(&program, "source_text_byte_at_or".to_string());

    assert!(acl.contains(
        "op create_function id=fn.byte return=Int body=text.byte_at_or(value, index, fallback)"
    ));
}

#[test]
fn lowers_source_text_parse_int_or_helper() {
    let program = parse_ail_source(
        r#"
fn parsed(value: Text, fallback: Int) -> Int = text_parse_int_or(value, fallback)
"#,
    )
    .expect("source text_parse_int_or must parse");
    let acl = source_program_to_acl(&program, "source_text_parse_int_or".to_string());

    assert!(acl.contains(
        "op create_function id=fn.parsed return=Int body=text.parse_int_or(value, fallback)"
    ));
}

#[test]
fn lowers_source_text_slice_helper() {
    let program = parse_ail_source(
        r#"
fn piece(value: Text, start: Int, length: Int) -> Text = text_slice(value, start, length)
"#,
    )
    .expect("source text_slice must parse");
    let acl = source_program_to_acl(&program, "source_text_slice".to_string());

    assert!(acl.contains(
        "op create_function id=fn.piece return=Text body=text.slice(value, start, length)"
    ));
}

#[test]
fn lowers_source_text_replace_first_helper() {
    let program = parse_ail_source(
            r#"
fn changed(value: Text, needle: Text, replacement: Text) -> Text = text_replace_first(value, needle, replacement)
"#,
        )
        .expect("source text_replace_first must parse");
    let acl = source_program_to_acl(&program, "source_text_replace_first".to_string());

    assert!(acl.contains(
            "op create_function id=fn.changed return=Text body=text.replace_first(value, needle, replacement)"
        ));
}

#[test]
fn lowers_source_text_boundary_helpers() {
    let program = parse_ail_source(
        r#"
fn prefixed(haystack: Text, prefix: Text) -> Bool = text_starts_with(haystack, prefix)
fn suffixed(haystack: Text, suffix: Text) -> Bool = text_ends_with(haystack, suffix)
"#,
    )
    .expect("source text boundary helpers must parse");
    let acl = source_program_to_acl(&program, "source_text_boundary".to_string());

    assert!(acl.contains(
        "op create_function id=fn.prefixed return=Bool body=text.starts_with(haystack, prefix)"
    ));
    assert!(acl.contains(
        "op create_function id=fn.suffixed return=Bool body=text.ends_with(haystack, suffix)"
    ));
}

#[test]
fn lowers_source_unary_minus() {
    let program = parse_ail_source(
        "fn negated(x: Int) -> Int = -x
test grouped = -(1 + 2) == -3",
    )
    .expect("source must parse");
    let acl = source_program_to_acl(&program, "source_negate".to_string());

    assert!(acl.contains("op create_function id=fn.negated return=Int body=sub(0, x)"));
    assert!(
        acl.contains("op create_test id=test.grouped return=Bool body=eq(sub(0, add(1, 2)), -3)")
    );
}

#[test]
fn lowers_source_params_to_acl_add_param_ops() {
    let program =
        parse_ail_source("fn add_pair(x: Int, y: Int) -> Int = add(x, y)").expect("source");
    let acl = source_program_to_acl(&program, "source_params".to_string());

    assert!(acl.contains("op create_function id=fn.add_pair return=Int body=add(x, y)"));
    assert!(acl.contains("op add_param target=fn.add_pair name=x type=Int"));
    assert!(acl.contains("op add_param target=fn.add_pair name=y type=Int"));
}

#[test]
fn lowers_source_typed_let_annotations() {
    let program = parse_ail_source(
        r#"
fn main() -> Int {
  let base: Int = 20 + 20
  return base + 2
}
"#,
    )
    .expect("source must parse");
    let acl = source_program_to_acl(&program, "source_typed_let".to_string());

    assert_eq!(
        program.functions[0].body,
        "let_typed(base, Int, 3, add(20, 20), add(base, 2))"
    );
    assert!(acl.contains(
        "op create_function id=fn.main return=Int body=let(base, add(20, 20), add(base, 2))"
    ));
}

#[test]
fn lowers_source_block_let_to_acl_body_expr() {
    let program = parse_ail_source(
        r#"
fn main() -> Int {
  let base = add(20, 20)
  return add(base, 2)
}
"#,
    )
    .expect("source");
    let acl = source_program_to_acl(&program, "source_block".to_string());

    assert!(acl.contains(
        "op create_function id=fn.main return=Int body=let(base, add(20, 20), add(base, 2))"
    ));
}

#[test]
fn lowers_source_if_expression_to_compiler_if_call() {
    let program = parse_ail_source(
        r#"
fn clamp_positive(x: Int) -> Int {
  if gt(x, 0) { x } else { 0 }
}
test clamp = eq(clamp_positive(-5), 0)
"#,
    )
    .expect("source");
    let acl = source_program_to_acl(&program, "source_if".to_string());

    assert!(
        acl.contains("op create_function id=fn.clamp_positive return=Int body=if(gt(x, 0), x, 0)")
    );
    assert!(
        acl.contains("op create_test id=test.clamp return=Bool body=eq(clamp_positive(-5), 0)")
    );
}

#[test]
fn lowers_source_capabilities_and_grants_to_acl_ops() {
    let program = parse_ail_source(
        r#"
capability log.write
fn print_hello() -> Int = print("Hello from source!")
grant print_hello log.write
"#,
    )
    .expect("source capability program must parse");
    let acl = source_program_to_acl(&program, "source_capability".to_string());

    assert!(acl.contains("op create_capability id=log.write"));
    assert!(acl.contains(
        r#"op create_function id=fn.print_hello return=Int body=print("Hello from source!")"#
    ));
    assert!(acl.contains("op grant target=fn.print_hello capability=log.write"));
}
