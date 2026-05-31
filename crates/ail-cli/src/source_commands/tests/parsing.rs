use super::*;

#[test]
fn parses_functions_and_tests_from_ail_source() {
    let program = parse_ail_source(
        r#"
// real source, not ACL
fn main() -> Int = add(20, 22)
fn add_pair(x: Int, y: Int) -> Int = add(x, y)
fn with_local() -> Int {
  let base = add(20, 20)
  if gt(base, 40) { add(base, 2) } else { 0 }
}
test main_addition = eq(add(20, 22), 42);
"#,
    )
    .expect("source must parse");

    assert_eq!(program.functions[0].name, "fn.main");
    assert_eq!(program.functions[0].return_type, "Int");
    assert_eq!(program.functions[1].name, "fn.add_pair");
    assert_eq!(program.functions[1].params[0].name, "x");
    assert_eq!(program.functions[1].params[0].ty, "Int");
    assert_eq!(program.functions[1].params[1].name, "y");
    assert_eq!(program.functions[1].params[1].ty, "Int");
    assert_eq!(program.functions[2].name, "fn.with_local");
    assert_eq!(
        program.functions[2].body,
        "let(base, add(20, 20), if(gt(base, 40), add(base, 2), 0))"
    );
    assert_eq!(program.tests[0].name, "test.main_addition");
    assert_eq!(program.tests[0].return_type, "Bool");
}

#[test]
fn rejects_duplicate_source_function_declarations() {
    let err = parse_ail_source(
        r#"
fn main() -> Int = 1
fn main() -> Int = 2
"#,
    )
    .expect_err("duplicate source functions must be rejected");

    assert!(
        err.to_string()
            .contains("duplicate function declaration `fn.main`")
    );
}

#[test]
fn rejects_empty_source_constructor_match_patterns_with_specific_error() {
    let err = parse_ail_source(
        r#"
fn main(input: Option<Int>) -> Int = match input { Some() => 1, None => 0 }
"#,
    )
    .expect_err("empty source constructor match patterns must be rejected");

    assert!(err.to_string().contains(
        "line 2: unsupported empty source match pattern `Some()`: constructor arms require a single local binding or `_`"
    ));
}

#[test]
fn rejects_source_list_destructuring_match_patterns_with_specific_error() {
    let err = parse_ail_source(
        r#"
fn main(input: Option<List<Int>>) -> Int = match input { Some([head, tail]) => head, None => 0 }
"#,
    )
    .expect_err("list destructuring source match patterns must be rejected");

    assert!(err.to_string().contains(
        "line 2: unsupported source list match pattern `Some([head, tail])`: constructor arms currently support only a single local binding or `_`; bind the value and inspect elements in the arm body"
    ));
}

#[test]
fn rejects_malformed_nested_source_types_with_specific_diagnostics() {
    let cases = [
        (
            r#"
fn bad(input: List<Map<Int,Bool>) -> Int = 1
"#,
            "line 2: unbalanced angle brackets in source type `List<Map<Int,Bool>`",
        ),
        (
            r#"
fn bad() -> Map<Text> = map("a", 1)
"#,
            "line 2: source type `Map` expects 2 type argument(s), got 1 in `Map<Text>`",
        ),
        (
            r#"
fn bad(input: Option<Result<Int>>) -> Int = 1
"#,
            "line 2: source type `Result` expects 2 type argument(s), got 1 in `Result<Int>`",
        ),
        (
            r#"
fn bad(input: Tuple<Int,,Bool>) -> Int = 1
"#,
            "line 2: source type `Tuple` has empty type argument at position 2 in `Tuple<Int,,Bool>`",
        ),
        (
            r#"
fn bad(input: Record<person:Record<age:Int>,nameText>) -> Int = 1
"#,
            "line 2: source type `Record` field `nameText` must use `field: Type` in `Record<person:Record<age:Int>,nameText>`",
        ),
    ];

    for (src, expected) in cases {
        let err = parse_ail_source(src).expect_err("malformed source type must be rejected");
        assert!(
            err.to_string().contains(expected),
            "expected diagnostic `{expected}`, got `{err}`"
        );
    }
}

#[test]
fn qualifies_source_module_declarations_and_local_calls() {
    let program = parse_ail_source(
        r#"
module math
fn add_pair(x: Int, y: Int) -> Int = add(x, y)
fn main() -> Int = add_pair(20, 22)
test main_addition = eq(main(), 42)
"#,
    )
    .expect("source module must parse");
    let acl = source_program_to_acl(&program, "source_module".to_string());
    let (formatted, item_count) = format_ail_source(
        r#"
module math
fn add_pair(x:Int,y:Int)->Int=add(x,y)
fn main()->Int=add_pair(20,22)
test main_addition=eq(main(),42)
"#,
    )
    .expect("source module must format");

    assert_eq!(program.module.as_deref(), Some("math"));
    assert!(acl.contains("op create_function id=fn.math.add_pair return=Int body=add(x, y)"));
    assert!(
        acl.contains("op create_function id=fn.math.main return=Int body=math.add_pair(20, 22)")
    );
    assert!(acl.contains(
        "op create_test id=test.math.main_addition return=Bool body=eq(math.main(), 42)"
    ));
    assert_eq!(item_count, 4);
    assert!(formatted.contains("module math\n"));
    assert!(formatted.contains("fn main() -> Int = add_pair(20, 22)\n"));
    assert!(formatted.contains("test main_addition = main() == 42\n"));
}

#[test]
fn parses_and_formats_relative_source_imports() {
    let src = r#"
use "./math.ail"
fn main() -> Int = add_pair(20, 22)
"#;
    let program = parse_ail_source(src).expect("source imports must parse");
    let acl = source_program_to_acl(&program, "source_import".to_string());
    let (formatted, item_count) = format_ail_source(src).expect("source must format");

    assert_eq!(program.imports, vec!["./math.ail".to_string()]);
    assert!(!acl.contains("use"));
    assert!(acl.contains("op create_function id=fn.main return=Int body=add_pair(20, 22)"));
    assert_eq!(item_count, 2);
    assert_eq!(
        formatted,
        "use \"./math.ail\"\nfn main() -> Int = add_pair(20, 22)\n"
    );
}

#[test]
fn preserves_string_literals_with_comment_markers_and_braces() {
    let program = parse_ail_source(
        r#"
fn message() -> Text = concat("Hello, //", " {world}")
fn choose(flag: Bool) -> Text = if flag { "left } brace" } else { "right // slash" }
"#,
    )
    .expect("source string literals must parse");
    let acl = source_program_to_acl(&program, "source_strings".to_string());

    assert!(acl.contains(
        r#"op create_function id=fn.message return=Text body=concat("Hello, //", " {world}")"#
    ));
    assert!(acl.contains(
            r#"op create_function id=fn.choose return=Text body=if(flag, "left } brace", "right // slash")"#
        ));
}
