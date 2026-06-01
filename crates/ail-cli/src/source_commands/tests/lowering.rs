use super::super::lower::*;
use super::*;

fn assert_lowering_diagnostic(
    result: Result<String, CliError>,
    code: &str,
    category: &str,
    detail: &str,
) -> String {
    let err = result.expect_err("source lowering must fail");
    let CliError::ParseError(message) = err else {
        panic!("source lowering must return ParseError");
    };
    assert!(
        message.contains(code),
        "lowering diagnostic must include stable code `{code}`; got: {message}"
    );
    assert!(
        message.contains(&format!("category={category}")),
        "lowering diagnostic must include category `{category}`; got: {message}"
    );
    assert!(
        message.contains(detail),
        "lowering diagnostic must keep actionable detail `{detail}`; got: {message}"
    );
    message
}

fn assert_graph_materialization_diagnostic<T: std::fmt::Debug>(
    result: Result<T, CliError>,
    code: &str,
    category: &str,
    detail: &str,
) -> String {
    let err = result.expect_err("source graph materialization must fail");
    let CliError::ParseError(message) = err else {
        panic!("source graph materialization must return ParseError");
    };
    assert!(
        message.contains(code),
        "graph materialization diagnostic must include stable code `{code}`; got: {message}"
    );
    assert!(
        message.contains(&format!("category={category}")),
        "graph materialization diagnostic must include category `{category}`; got: {message}"
    );
    assert!(
        message.contains(detail),
        "graph materialization diagnostic must keep actionable detail `{detail}`; got: {message}"
    );
    message
}

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
fn rejects_invalid_acl_materialization_with_stable_diagnostic() {
    assert_graph_materialization_diagnostic(
        source_program_to_graph(&SourceProgram::default(), "source\ninvalid"),
        "AIL_SOURCE_LOWER_TO_ACL",
        "source.lower.to_acl",
        "failed to lower AIL source to ACL",
    );
}

#[test]
fn rejects_collection_helper_arity_with_stable_lowering_diagnostic() {
    assert_lowering_diagnostic(
        lower_source_expr("first_or(values)", 12),
        "AIL_SOURCE_LOWER_COLLECTION_ARITY",
        "source.lower.collection",
        "first_or requires `first_or(list, fallback)`",
    );
}

#[test]
fn rejects_collection_literals_with_stable_lowering_diagnostics() {
    assert_lowering_diagnostic(
        lower_source_expr("[1, 2", 7),
        "AIL_SOURCE_LOWER_LIST_LITERAL",
        "source.lower.collection",
        "list literal has unclosed `[`",
    );
    assert_lowering_diagnostic(
        lower_source_expr("{ age 42 }", 8),
        "AIL_SOURCE_LOWER_RECORD_LITERAL",
        "source.lower.collection",
        "record literal field requires `name: expression`",
    );
}

#[test]
fn rejects_accessor_and_operator_shapes_with_stable_lowering_diagnostics() {
    assert_lowering_diagnostic(
        lower_source_expr("values[]", 20),
        "AIL_SOURCE_LOWER_INDEX_EXPRESSION",
        "source.lower.collection",
        "index expression requires `collection[index]`",
    );
    assert_lowering_diagnostic(
        lower_source_expr("-", 21),
        "AIL_SOURCE_LOWER_UNARY_OPERATOR",
        "source.lower.operator",
        "unary `-` requires an expression",
    );
}

#[test]
fn rejects_unsupported_constructs_with_redacted_lowering_descriptor() {
    let diagnostic = assert_lowering_diagnostic(
        lower_source_expr("for customer_secret in values { customer_secret }", 31),
        "AIL_SOURCE_LOWER_UNSUPPORTED_CONSTRUCT",
        "source.lower.unsupported",
        "unsupported source construct `for` during lowering",
    );

    assert!(
        diagnostic.contains("descriptor={line=31,construct=for,sourceLength=49,sourceHash="),
        "lowering diagnostic must include a deterministic redacted descriptor; got: {diagnostic}"
    );
    assert!(
        !diagnostic.contains("customer_secret"),
        "lowering diagnostic descriptor must not leak raw source; got: {diagnostic}"
    );
    assert_eq!(
        diagnostic,
        assert_lowering_diagnostic(
            lower_source_expr("for customer_secret in values { customer_secret }", 31),
            "AIL_SOURCE_LOWER_UNSUPPORTED_CONSTRUCT",
            "source.lower.unsupported",
            "unsupported source construct `for` during lowering",
        ),
        "same lowering failure must produce deterministic diagnostics"
    );
}

#[test]
fn rejects_typed_let_type_shape_during_lowering() {
    let diagnostic = assert_lowering_diagnostic(
        lower_source_expr("let_typed(value, List<Int, Text>, 9, 1, value)", 9),
        "AIL_SOURCE_LOWER_TYPE_SHAPE",
        "source.lower.type",
        "typed let annotation has unsupported source type shape",
    );

    assert!(
        diagnostic.contains("descriptor={line=9,construct=typed_let,"),
        "typed let type-shape mismatch must include a redacted lowering descriptor; got: {diagnostic}"
    );
    assert!(
        !diagnostic.contains("List<Int, Text>"),
        "type-shape descriptor should not echo the raw source expression; got: {diagnostic}"
    );
}

#[test]
fn rejects_duplicate_record_fields_before_losing_bindings() {
    assert_lowering_diagnostic(
        lower_source_expr(r#"{ age: 1, name: "Ada", age: 2 }"#, 14),
        "AIL_SOURCE_LOWER_BINDING_SHAPE",
        "source.lower.binding",
        "duplicate record field `age` would overwrite an earlier binding",
    );
}

#[test]
fn rejects_missing_effect_capability_reference_during_lowering() {
    let diagnostic = assert_lowering_diagnostic(
        lower_source_expr("effect_call(, write, customer_secret)", 17),
        "AIL_SOURCE_LOWER_CAPABILITY_REFERENCE",
        "source.lower.capability",
        "effect_call requires `effect_call(capability, operation, ...)`",
    );

    assert!(
        diagnostic.contains("descriptor={line=17,construct=effect_call,"),
        "capability reference diagnostic must include a redacted descriptor; got: {diagnostic}"
    );
    assert!(
        !diagnostic.contains("customer_secret"),
        "capability reference descriptor must not leak raw source; got: {diagnostic}"
    );
}

#[test]
fn rejects_log_write_arity_with_stable_lowering_diagnostic() {
    assert_lowering_diagnostic(
        lower_source_expr(r#"log.write("hello", "extra")"#, 18),
        "AIL_SOURCE_LOWER_EXPRESSION",
        "source.lower.expression",
        "log.write requires `log.write(message)`",
    );
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
fn rejects_tuple_helper_arity_with_stable_lowering_diagnostic() {
    assert_lowering_diagnostic(
        lower_source_expr("tuple_first(pair(), 1)", 17),
        "AIL_SOURCE_LOWER_TUPLE_HELPER",
        "source.lower.tuple",
        "tuple_first requires `tuple_first(tuple)`",
    );
}

#[test]
fn lowers_source_tuple_accessors_to_core_calls() {
    let program = parse_ail_source(
        r#"
fn pair() -> Tuple<Int, Text> = tuple(42, "answer")
fn first() -> Option<Int> = tuple_first(pair())
fn second() -> Option<Text> = tuple.second(pair())
fn pair_len() -> Int = tuple_length(pair())
fn pair_get() -> Option<Text> = tuple_get(pair(), 1)
"#,
    )
    .expect("source tuple accessors must parse");
    let acl = source_program_to_acl(&program, "source_tuple_accessors".to_string());

    assert!(
        acl.contains("op create_function id=fn.first return=Option<Int> body=tuple.first(pair())")
    );
    assert!(
        acl.contains(
            "op create_function id=fn.second return=Option<Text> body=tuple.second(pair())"
        )
    );
    assert!(acl.contains("op create_function id=fn.pair_len return=Int body=tuple.length(pair())"));
    assert!(acl.contains(
        "op create_function id=fn.pair_get return=Option<Text> body=tuple.get(pair(), 1)"
    ));
}

#[test]
fn lowers_source_list_mutation_helpers_to_core_calls() {
    let program = parse_ail_source(
        r#"
fn values() -> List<Int> = list(1, 2)
fn pushed() -> List<Int> = list_push(values(), 3)
fn merged() -> List<Int> = list_concat(values(), list(3, 4))
"#,
    )
    .expect("source list helpers must parse");
    let acl = source_program_to_acl(&program, "source_list_helpers".to_string());

    assert!(
        acl.contains(
            "op create_function id=fn.pushed return=List<Int> body=list.push(values(), 3)"
        )
    );
    assert!(acl.contains(
        "op create_function id=fn.merged return=List<Int> body=list.concat(values(), list(3, 4))"
    ));
}

#[test]
fn lowers_source_queue_helpers_to_core_calls() {
    let program = parse_ail_source(
        r#"
fn queue() -> List<Int> = list(1, 2)
fn pushed() -> List<Int> = queue_push_back(queue(), 3)
fn popped() -> Option<Tuple<Int, List<Int>>> = queue_pop_front(queue())
fn peeked() -> Option<Int> = queue_peek_front(queue())
fn count() -> Int = queue_length(queue())
fn empty() -> Bool = queue_is_empty(queue())
"#,
    )
    .expect("source queue helpers must parse");
    let acl = source_program_to_acl(&program, "source_queue_helpers".to_string());

    assert!(acl.contains(
        "op create_function id=fn.pushed return=List<Int> body=queue.push_back(queue(), 3)"
    ));
    assert!(acl.contains("op create_function id=fn.popped return=Option<Tuple<Int,List<Int>>> body=queue.pop_front(queue())"));
    assert!(acl.contains(
        "op create_function id=fn.peeked return=Option<Int> body=queue.peek_front(queue())"
    ));
    assert!(acl.contains("op create_function id=fn.count return=Int body=queue.length(queue())"));
    assert!(
        acl.contains("op create_function id=fn.empty return=Bool body=queue.is_empty(queue())")
    );
}

#[test]
fn lowers_source_set_helpers_to_core_calls() {
    let program = parse_ail_source(
        r#"
fn ids() -> Set<Int> = set(1, 2)
fn has_two() -> Bool = set_contains(ids(), 2)
fn count() -> Int = set_length(ids())
fn updated() -> Set<Int> = set_insert(ids(), 3)
"#,
    )
    .expect("source set helpers must parse");
    let acl = source_program_to_acl(&program, "source_set_helpers".to_string());

    assert!(
        acl.contains("op create_function id=fn.has_two return=Bool body=set.contains(ids(), 2)")
    );
    assert!(acl.contains("op create_function id=fn.count return=Int body=set.length(ids())"));
    assert!(
        acl.contains("op create_function id=fn.updated return=Set<Int> body=set.insert(ids(), 3)")
    );
}

#[test]
fn lowers_source_map_helpers_to_core_calls() {
    let program = parse_ail_source(
        r#"
fn labels() -> Map<Text, Int> = map("one", 1)
fn maybe() -> Option<Int> = map_get(labels(), "one")
fn has() -> Bool = map_contains_key(labels(), "one")
fn count() -> Int = map_length(labels())
fn updated() -> Map<Text, Int> = map_insert(labels(), "two", 2)
"#,
    )
    .expect("source map helpers must parse");
    let acl = source_program_to_acl(&program, "source_map_helpers".to_string());

    assert!(acl.contains(
        r#"op create_function id=fn.maybe return=Option<Int> body=map.get(labels(), "one")"#
    ));
    assert!(acl.contains(
        r#"op create_function id=fn.has return=Bool body=map.contains_key(labels(), "one")"#
    ));
    assert!(acl.contains("op create_function id=fn.count return=Int body=map.length(labels())"));
    assert!(acl.contains(r#"op create_function id=fn.updated return=Map<Text,Int> body=map.insert(labels(), "two", 2)"#));
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
fn lowers_source_option_result_conversion_helpers() {
    let program = parse_ail_source(
        r#"
fn value(input: Result<Int, Text>) -> Int = result_unwrap_or(input, 0)
fn promoted(input: Option<Int>) -> Result<Int, Text> = ok_or(input, "missing")
fn option_value(input: Option<Int>) -> Int = option_unwrap_or(input, 1)
"#,
    )
    .expect("source option/result conversion helpers must parse");
    let acl = source_program_to_acl(&program, "source_option_result_helpers".to_string());

    assert!(acl.contains(
            "op create_function id=fn.value return=Int body=match(input, Ok(__ail_unwrap), __ail_unwrap, Err(_), 0)"
        ));
    assert!(acl.contains(
            r#"op create_function id=fn.promoted return=Result<Int,Text> body=match(input, Some(__ail_ok), ok(__ail_ok), None, err("missing"))"#
        ));
    assert!(acl.contains(
            "op create_function id=fn.option_value return=Int body=match(input, Some(__ail_unwrap), __ail_unwrap, None, 1)"
        ));
}

#[test]
fn rejects_option_result_helper_arity_with_stable_lowering_diagnostic() {
    assert_lowering_diagnostic(
        lower_source_expr("option_is_some()", 33),
        "AIL_SOURCE_LOWER_OPTION_RESULT_HELPER",
        "source.lower.option_result",
        "option_is_some requires `option_is_some(option)`",
    );
}

#[test]
fn lowers_source_option_predicate_helpers() {
    let program = parse_ail_source(
        r#"
fn has_value(input: Option<Int>) -> Bool = is_some(input)
fn missing(input: Option<Int>) -> Bool = option_is_none(input)
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
fn failed(input: Result<Int, Text>) -> Bool = result.is_err(input)
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
fn lowers_source_list_get_helper() {
    let program = parse_ail_source(
        r#"
fn maybe_item(values: List<Int>, idx: Int) -> Option<Int> = list_get(values, idx)
"#,
    )
    .expect("source list_get must parse");
    let acl = source_program_to_acl(&program, "source_list_get".to_string());

    assert!(acl.contains(
            "op create_function id=fn.maybe_item return=Option<Int> body=if(and(ge(idx, 0), lt(idx, len(values))), some(index(values, idx)), none())"
        ));
}

#[test]
fn lowers_source_is_empty_helper() {
    let program = parse_ail_source(
        r#"
fn no_items(values: List<Int>) -> Bool = is_empty(values)
fn no_text(value: Text) -> Bool = is_empty(value)
fn no_items_named(values: List<Int>) -> Bool = list.is_empty(values)
fn no_text_named(value: Text) -> Bool = text_is_empty(value)
"#,
    )
    .expect("source is_empty must parse");
    let acl = source_program_to_acl(&program, "source_is_empty".to_string());

    assert!(acl.contains("op create_function id=fn.no_items return=Bool body=eq(len(values), 0)"));
    assert!(acl.contains("op create_function id=fn.no_text return=Bool body=eq(len(value), 0)"));
    assert!(
        acl.contains("op create_function id=fn.no_items_named return=Bool body=eq(len(values), 0)")
    );
    assert!(
        acl.contains("op create_function id=fn.no_text_named return=Bool body=eq(len(value), 0)")
    );
}

#[test]
fn lowers_source_length_alias_helpers() {
    let program = parse_ail_source(
        r#"
fn text_len(value: Text) -> Int = text_length(value)
fn text_len_dotted(value: Text) -> Int = text.length(value)
fn list_len(values: List<Int>) -> Int = list_length(values)
fn list_len_dotted(values: List<Int>) -> Int = list.length(values)
"#,
    )
    .expect("source length aliases must parse");
    let acl = source_program_to_acl(&program, "source_length_aliases".to_string());

    assert!(acl.contains("op create_function id=fn.text_len return=Int body=len(value)"));
    assert!(acl.contains("op create_function id=fn.text_len_dotted return=Int body=len(value)"));
    assert!(acl.contains("op create_function id=fn.list_len return=Int body=len(values)"));
    assert!(acl.contains("op create_function id=fn.list_len_dotted return=Int body=len(values)"));
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
fn lowers_source_pipe_operator_into_first_argument_calls() {
    let program = parse_ail_source(
        r#"
fn cleaned(value: Text) -> Text = value |> text_trim() |> text_replace_first(" ", "_")
fn bounded(value: Int) -> Int = value |> int_clamp(0, 10)
fn custom(value: Int) -> Int = value |> normalize
"#,
    )
    .expect("source pipe operator must parse");
    let acl = source_program_to_acl(&program, "source_pipe".to_string());

    assert!(acl.contains(
        r#"op create_function id=fn.cleaned return=Text body=text.replace_first(text.trim(value), " ", "_")"#
    ));
    assert!(
        acl.contains("op create_function id=fn.bounded return=Int body=int.clamp(value, 0, 10)")
    );
    assert!(acl.contains("op create_function id=fn.custom return=Int body=normalize(value)"));
}

#[test]
fn rejects_pipe_operator_target_shape_with_stable_diagnostic() {
    let diagnostic = assert_lowering_diagnostic(
        lower_source_expr("customer_secret |> 42", 24),
        "AIL_SOURCE_LOWER_PIPE_EXPRESSION",
        "source.lower.pipe",
        "pipe target requires an identifier or function call",
    );

    assert!(
        diagnostic.contains("descriptor={line=24,construct=pipe,sourceLength=2,sourceHash="),
        "pipe diagnostic must include a redacted descriptor for the target; got: {diagnostic}"
    );
    assert!(
        !diagnostic.contains("customer_secret"),
        "pipe diagnostic must not leak the piped value; got: {diagnostic}"
    );
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
fn rejects_int_helper_arity_with_stable_lowering_diagnostic() {
    assert_lowering_diagnostic(
        lower_source_expr("int_clamp(value)", 29),
        "AIL_SOURCE_LOWER_INT_HELPER",
        "source.lower.int",
        "int_clamp requires `int_clamp(value, low, high)`",
    );
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
fn rejects_text_helper_arity_with_stable_lowering_diagnostic() {
    assert_lowering_diagnostic(
        lower_source_expr("text_contains(value)", 42),
        "AIL_SOURCE_LOWER_TEXT_HELPER",
        "source.lower.text",
        "text_contains requires `text_contains(haystack, needle)`",
    );
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
fn rejects_match_expression_shape_with_stable_lowering_diagnostic() {
    assert_lowering_diagnostic(
        lower_source_expr("match { _ => 1 }", 61),
        "AIL_SOURCE_LOWER_MATCH_EXPRESSION",
        "source.lower.match",
        "match expression requires a scrutinee",
    );
}

#[test]
fn rejects_if_expression_shape_with_stable_lowering_diagnostic() {
    assert_lowering_diagnostic(
        lower_source_expr("if { 1 } else { 2 }", 55),
        "AIL_SOURCE_LOWER_CONTROL_EXPRESSION",
        "source.lower.control",
        "if expression requires a condition",
    );
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
fn lowers_source_else_if_expression_to_nested_compiler_if_call() {
    let program = parse_ail_source(
        r#"
fn bucket(x: Int) -> Int {
  if gt(x, 10) { 3 } else if gt(x, 0) { 2 } else { 1 }
}
test bucket_negative = eq(bucket(-5), 1)
"#,
    )
    .expect("source");
    let acl = source_program_to_acl(&program, "source_else_if".to_string());

    assert!(acl.contains(
        "op create_function id=fn.bucket return=Int body=if(gt(x, 10), 3, if(gt(x, 0), 2, 1))"
    ));
    assert!(
        acl.contains("op create_test id=test.bucket_negative return=Bool body=eq(bucket(-5), 1)")
    );
}

#[test]
fn lowers_multiline_source_if_else_if_blocks_to_nested_compiler_if_call() {
    let program = parse_ail_source(
        r#"
fn bucket(x: Int) -> Int {
  if gt(x, 10) {
    3
  } else if gt(x, 0) {
    2
  } else {
    1
  }
}
test bucket_negative = eq(bucket(-5), 1)
"#,
    )
    .expect("multiline source if must parse");
    let acl = source_program_to_acl(&program, "source_multiline_if".to_string());

    assert_eq!(
        program.functions[0].body,
        "if(gt(x, 10), 3, if(gt(x, 0), 2, 1))"
    );
    assert!(acl.contains(
        "op create_function id=fn.bucket return=Int body=if(gt(x, 10), 3, if(gt(x, 0), 2, 1))"
    ));
    assert!(
        acl.contains("op create_test id=test.bucket_negative return=Bool body=eq(bucket(-5), 1)")
    );
}

#[test]
fn lowers_return_markers_inside_source_if_branches() {
    let program = parse_ail_source(
        r#"
fn bucket(x: Int) -> Int {
  if gt(x, 10) {
    return 3
  } else if gt(x, 0) {
    return 2
  } else {
    return 1
  }
}
test bucket_negative = eq(bucket(-5), 1)
"#,
    )
    .expect("source if branches may use return markers");
    let acl = source_program_to_acl(&program, "source_if_returns".to_string());

    assert_eq!(
        program.functions[0].body,
        "if(gt(x, 10), 3, if(gt(x, 0), 2, 1))"
    );
    assert!(acl.contains(
        "op create_function id=fn.bucket return=Int body=if(gt(x, 10), 3, if(gt(x, 0), 2, 1))"
    ));
}

#[test]
fn lowers_source_capabilities_and_grants_to_acl_ops() {
    let program = parse_ail_source(
        r#"
capability log.write
fn print_hello() -> Int = print("Hello from source!")
fn log_hello() -> Int = log.write("Hello from alias!")
grant print_hello log.write
grant log_hello log.write
"#,
    )
    .expect("source capability program must parse");
    let acl = source_program_to_acl(&program, "source_capability".to_string());

    assert!(acl.contains("op create_capability id=log.write"));
    assert!(acl.contains(
        r#"op create_function id=fn.print_hello return=Int body=print("Hello from source!")"#
    ));
    assert!(acl.contains(
        r#"op create_function id=fn.log_hello return=Int body=print("Hello from alias!")"#
    ));
    assert!(acl.contains("op grant target=fn.print_hello capability=log.write"));
    assert!(acl.contains("op grant target=fn.log_hello capability=log.write"));
}
