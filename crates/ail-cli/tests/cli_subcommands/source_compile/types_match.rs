// Mechanical phase 2 split from source_compile.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use crate::common::parse_json_output;
use predicates::prelude::*;

#[test]
fn compile_file_rejects_source_int_bounds_helper_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_int_bounds.ail");
    source
        .write_str(
            "fn bounded(value: Int, fallback: Text) -> Int = int_shift_right(value, fallback)\n",
        )
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in int.shift_right_unsigned argument 2: expected Int, got Text",
        ));
}
#[test]
fn compile_file_accepts_source_option_result_constructors() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("option_result.ail");
    source
        .write_str(
            "fn maybe(flag: Bool) -> Option<Int> = if flag { Some(42) } else { None }\n\
fn ok_value() -> Result<Int, Text> = Ok(42)\n\
fn err_value() -> Result<Int, Text> = Err(\"boom\")\n",
        )
        .expect("source fixture must be written");

    let output = ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .args(["--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let json = parse_json_output(&output);
    assert_eq!(
        json["data"]["export_types"]["maybe"],
        serde_json::json!({ "Option": { "Scalar": "I64" } })
    );
    assert_eq!(
        json["data"]["export_types"]["ok_value"],
        serde_json::json!({ "Result": { "ok": { "Scalar": "I64" }, "err": "Text" } })
    );
    assert_eq!(
        json["data"]["export_types"]["err_value"],
        serde_json::json!({ "Result": { "ok": { "Scalar": "I64" }, "err": "Text" } })
    );
}
#[test]
fn compile_file_accepts_source_type_aliases() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("type_aliases.ail");
    source
        .write_str(
            "fn text_len(value: String) -> int = len(value)\n\
fn first(values: List<i64>) -> i64 = values[0]\n\
fn ids() -> Set<i64> = set(1, 2)\n\
fn labels() -> Map<String, int> = map(\"one\", 1)\n\
fn pair() -> Tuple<i64, String> = tuple(42, \"answer\")\n\
fn person() -> Record<age: i64, name: String> = record(age, 42, name, \"Ada\")\n\
fn maybe(flag: bool) -> Option<i32> = if flag { Some(42) } else { None }\n\
fn result() -> Result<i64, String> = Ok(42)\n",
        )
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .success();
}
#[test]
fn compile_file_rejects_source_option_result_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_option.ail");
    source
        .write_str("fn main() -> Option<Int> = Some(true)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in fn.main: expected Option<Int>, got Option<Bool>",
        ));
}

#[test]
fn compile_file_rejects_source_option_result_constructor_arity_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_constructor.ail");
    source
        .write_str("fn main() -> Option<Int> = Some()\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "function call `Some` expects 1 argument(s), got 0",
        ));
}

#[test]
fn compile_file_accepts_source_match_expressions() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("match.ail");
    source
        .write_str(
            "fn unwrap_or_zero(value: Option<Int>) -> Int = match value { Some(v) => v, None => 0 }\n\
fn result_or_zero(value: Result<Int, Text>) -> Int = match value { Ok(v) => v, Err(e) => 0 }\n",
        )
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .success();
}
#[test]
fn compile_file_accepts_source_unwrap_or_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("unwrap_or.ail");
    source
        .write_str("fn value(input: Option<Int>) -> Int = unwrap_or(input, 0)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .success();
}
#[test]
fn compile_file_accepts_source_option_result_fallback_aliases() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("fallback_aliases.ail");
    source
        .write_str(
            "fn option_value(input: Option<Int>) -> Int = option_unwrap_or(input, 1)\n\
fn promoted(input: Option<Int>) -> Result<Int, Text> = option_ok_or(input, \"missing\")\n\
fn result_value(input: Result<Int, Text>) -> Int = result_unwrap_or(input, 2)\n\
fn dotted_option(input: Option<Int>) -> Int = option.unwrap_or(input, 3)\n\
fn dotted_promoted(input: Option<Int>) -> Result<Int, Text> = option.ok_or(input, \"missing\")\n\
fn dotted_result(input: Result<Int, Text>) -> Int = result.unwrap_or(input, 4)\n",
        )
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .success();
}
#[test]
fn compile_file_rejects_source_unwrap_or_fallback_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_unwrap_or.ail");
    source
        .write_str("fn value(input: Option<Int>) -> Int = unwrap_or(input, true)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in match arms: expected Int, got Bool",
        ));
}
#[test]
fn compile_file_accepts_source_option_predicate_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("option_predicates.ail");
    source
        .write_str(
            "fn has_value(input: Option<Int>) -> Bool = is_some(input)\n\
fn missing(input: Option<Int>) -> Bool = option_is_none(input)\n\
fn namespaced(input: Option<Int>) -> Bool = option.is_some(input)\n\
fn dotted_missing(input: Option<Int>) -> Bool = option.is_none(input)\n",
        )
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .success();
}
#[test]
fn compile_file_rejects_source_option_predicate_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_option_predicate.ail");
    source
        .write_str("fn has_value(input: Result<Int, Text>) -> Bool = is_some(input)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in is_some argument 1: expected Option<Unknown>, got Result<Int,Text>",
        ));
}
#[test]
fn compile_file_accepts_source_result_predicate_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("result_predicates.ail");
    source
        .write_str(
            "fn succeeded(input: Result<Int, Text>) -> Bool = is_ok(input)\n\
fn failed(input: Result<Int, Text>) -> Bool = result_is_err(input)\n\
fn namespaced(input: Result<Int, Text>) -> Bool = result.is_ok(input)\n\
fn dotted_failed(input: Result<Int, Text>) -> Bool = result.is_err(input)\n",
        )
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .success();
}
#[test]
fn compile_file_rejects_source_result_predicate_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_result_predicate.ail");
    source
        .write_str("fn succeeded(input: Option<Int>) -> Bool = is_ok(input)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in is_ok argument 1: expected Result<Unknown,Unknown>, got Option<Int>",
        ));
}
#[test]
fn compile_file_rejects_nested_source_match_pattern() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_nested_match.ail");
    source
        .write_str(
            "fn main(value: Option<Result<Int, Text>>) -> Int = match value { Some(Ok(v)) => v, None => 0 }\n",
        )
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unsupported nested source match pattern `Some(Ok(v))`",
        ));
}
#[test]
fn compile_file_rejects_source_match_arm_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_match.ail");
    source
        .write_str(
            "fn main(value: Option<Int>) -> Int = match value { Some(v) => v, None => true }\n",
        )
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in match arms: expected Int, got Bool",
        ));
}
#[test]
fn compile_file_rejects_non_exhaustive_source_match() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_match_exhaustive.ail");
    source
        .write_str("fn main(value: Option<Int>) -> Int = match value { Some(v) => v }\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "non-exhaustive match for Option<Int>: expected Some and None arms or `_`",
        ));
}
#[test]
fn compile_file_rejects_unreachable_source_match_arm() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_match_unreachable.ail");
    source
        .write_str("fn main(value: Option<Int>) -> Int = match value { _ => 0, Some(v) => v }\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unreachable match arm `Some(v)` after wildcard `_`",
        ));
}
#[test]
fn compile_file_rejects_duplicate_source_match_arm() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_match_duplicate.ail");
    source
        .write_str(
            "fn main(value: Option<Int>) -> Int = match value { Some(v) => v, Some(other) => other, None => 0 }\n",
        )
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "duplicate match arm pattern `Some`",
        ));
}
#[test]
fn compile_file_rejects_source_typed_let_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_typed_let.ail");
    source
        .write_str("fn main() -> Int {\n  let base: Bool = 20 + 20\n  return 0\n}\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "line 2: type mismatch in let binding base: expected Bool, got Int",
        ));
}
#[test]
fn compile_file_rejects_float_literal_return_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_float.ail");
    source
        .write_str("fn main() -> Int = 1.5\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "line 1: type mismatch in fn.main: expected Int, got Float",
        ));
}
