// Mechanical phase 2 split from source_compile.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use crate::common::parse_json_output;
use predicates::prelude::*;

#[test]
fn compile_file_accepts_source_typed_let_annotations() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("typed_let.ail");
    source
        .write_str("fn main() -> Int {\n  let base: Int = 20 + 20\n  return base + 2\n}\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .success();
}
#[test]
fn compile_file_accepts_source_consts() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("consts.ail");
    source
        .write_str(
            "const answer: Int = 40 + 2\nfn main() -> Int = answer\ntest answer = answer == 42\n",
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
fn compile_file_accepts_source_int_bounds_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("int_bounds.ail");
    source
        .write_str(
            "fn bounded(value: Int, low: Int, high: Int, fallback: Int) -> Int = int_min(value, high) + int_max(value, low) + int_clamp(value, low, high) + int_abs_or(value, 0) + int_neg_or(value, fallback) + int_add_or(value, 1, fallback) + int_sub_or(value, 1, fallback) + int_mul_or(value, 1, fallback) + int_saturating_add(value, 1) + int_saturating_sub(value, 1) + int_saturating_mul(value, 1) + int_saturating_neg(value) + int_wrapping_add(value, 1) + int_wrapping_sub(value, 1) + int_wrapping_mul(value, 1) + int_wrapping_neg(value) + int_bit_and(value, high) + int_bit_or(value, low) + int_bit_xor(value, high) + int_bit_not(value) + int_shift_left(value, 1) + int_shift_right(value, 1) + int_shift_right_unsigned(value, 1) + int_div_or(value, 1, fallback) + int_rem_or(value, 1, fallback)\n",
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
fn compile_file_rejects_malformed_source_constructor() {
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
            "source constructor `Some` requires exactly one value",
        ));
}

#[test]
fn compile_file_reports_redacted_source_lowering_diagnostic() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_lowering.ail");
    source
        .write_str("fn main() -> Int = for customer_secret in values { customer_secret }\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .get_output()
        .clone();
    let stderr = String::from_utf8(output.stderr).expect("stderr must be UTF-8");

    assert!(
        stderr.contains("AIL_SOURCE_LOWER_UNSUPPORTED_CONSTRUCT"),
        "stderr must include the stable source-lowerer diagnostic code; got: {stderr}"
    );
    assert!(
        stderr.contains("category=source.lower.unsupported"),
        "stderr must include source-lowerer diagnostic category; got: {stderr}"
    );
    assert!(
        stderr.contains("descriptor={line=1,construct=for,sourceLength=49,sourceHash="),
        "stderr must include a redacted source descriptor; got: {stderr}"
    );
    assert!(
        !stderr.contains("customer_secret"),
        "stderr must not echo raw source details in lowerer descriptors; got: {stderr}"
    );
}
#[test]
fn compile_file_compiles_ail_source_without_acl_authoring() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let math = dir.child("math.ail");
    math.write_str("fn add_pair(x: Int, y: Int) -> Int = x + y\n")
        .expect("imported source fixture must be written");
    let source = dir.child("main.ail");
    source
        .write_str("use \"./math.ail\"\nfn main() -> Int = add_pair(20, 22)\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .args(["--profile", "dev", "--target", "wasm", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["target"], "wasm");
    assert!(v["data"]["wasm_hash"].is_string());
    assert!(
        v["data"]["source_file"]
            .as_str()
            .expect("source_file must be present")
            .ends_with("main.ail")
    );
}
#[test]
fn compile_file_rejects_non_finite_source_numeric_literals() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_float.ail");
    source
        .write_str("fn main() -> Float = NaN\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unsupported source numeric literal `NaN`",
        ));
}
#[test]
fn compile_file_accepts_source_infix_addition() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("infix_add.ail");
    source
        .write_str("fn main() -> Int = 1 + 2\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .success();
}
#[test]
fn compile_file_accepts_source_infix_arithmetic_precedence() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("infix_arithmetic.ail");
    source
        .write_str("test math = 10 - 2 * 3 + (8 / 4 + 7 % 4) == 9\nfn main() -> Int = 0\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .success();
}
#[test]
fn compile_file_accepts_source_unary_minus() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("unary_minus.ail");
    source
        .write_str(
            "fn negated(x: Int) -> Int = -x
test grouped = -(1 + 2) == -3
fn main() -> Int = negated(3)
",
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
fn compile_file_accepts_source_infix_equality() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("infix_eq.ail");
    source
        .write_str(
            "test addition = 1 + 2 == 3\ntest different = 1 + 2 != 4\nfn main() -> Int = 0\n",
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
fn compile_file_accepts_source_infix_ordering() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("infix_order.ail");
    source
        .write_str(
            "test gt_case = 3 > 2\n\
test ge_case = 3 >= 3\n\
test lt_case = 2 < 3\n\
test le_case = 2 <= 2\n\
fn main() -> Int = 0\n",
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
fn compile_file_accepts_source_infix_boolean_logic() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("infix_logic.ail");
    source
        .write_str(
            "test combined = 3 > 2 && 2 < 3 || false\n\
fn main() -> Int = 0\n",
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
fn compile_file_accepts_source_unary_not_and_grouping() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("not_grouping.ail");
    source
        .write_str(
            "test grouped = !(3 > 2 && false) == true\n\
fn main() -> Int = 0\n",
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
fn compile_file_rejects_unsupported_source_expression_syntax() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_expr.ail");
    source
        .write_str("fn main() -> Int = 1 ** 2\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unsupported source expression `1 ** 2`",
        ));
}
#[test]
fn compile_file_rejects_unsupported_source_string_escapes() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_escape.ail");
    source
        .write_str("fn main() -> Text = \"bad \\q escape\"\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("malformed string literal"));
}
#[test]
fn compile_file_rejects_malformed_source_string_literals() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_string.ail");
    source
        .write_str("fn main() -> Text = \"unterminated\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("malformed string literal"));
}
#[test]
fn compile_file_rejects_untyped_source_builtins() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_fold.ail");
    source
        .write_str("fn main() -> Int = fold(0, [1], 0)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unsupported source builtin `fold` has no type inference",
        ));
}
#[test]
fn compile_file_rejects_source_functions_that_shadow_builtins() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("shadow_builtin.ail");
    source
        .write_str("fn add(x: Int) -> Int = x\nfn main() -> Int = add(1)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "function declaration `fn.add` shadows builtin `add`",
        ));
}
#[test]
fn compile_file_rejects_duplicate_source_parameters() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("duplicate_params.ail");
    source
        .write_str("fn main(x: Int, x: Int) -> Int = x\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("duplicate parameter `x`"));
}
#[test]
fn compile_file_rejects_dotted_source_parameter_names() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("dotted_param.ail");
    source
        .write_str("fn main(x.y: Int) -> Int = x.y\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "local binding name `x.y` must not contain `.`",
        ));
}
#[test]
fn compile_file_rejects_dotted_source_let_names() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("dotted_let.ail");
    source
        .write_str("fn main() -> Int {\n  let x.y = 1\n  return x.y\n}\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "local binding name `x.y` must not contain `.`",
        ));
}
#[test]
fn compile_file_rejects_numeric_leading_source_names() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("numeric_name.ail");
    source
        .write_str("fn 1bad() -> Int = 1\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "declaration name `1bad` segment `1bad` must start with a letter or `_`",
        ));
}
#[test]
fn compile_file_rejects_malformed_source_names() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_name.ail");
    source
        .write_str("fn bad..name() -> Int = 1\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "declaration name `bad..name` contains an empty path segment",
        ));
}
#[test]
fn compile_file_rejects_unsupported_source_return_type() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("unsupported_type.ail");
    source
        .write_str("fn main() -> Mystery = 1\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unsupported source type `Mystery`",
        ));
}
