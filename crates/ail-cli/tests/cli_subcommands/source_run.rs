// Mechanical split from cli_subcommands.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use crate::common::parse_json_output;
use predicates::prelude::*;

#[test]
fn run_file_executes_ail_source_main_without_acl_authoring() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn main() -> Int = add(20, 22)\n\
test main_addition = eq(add(20, 22), 42)\n",
        )
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("source:"))
        .stdout(predicate::str::contains("module: fn.main"))
        .stdout(predicate::str::contains("result: 42"));
}
#[test]
fn run_file_executes_ail_source_function_with_typed_params() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("math.ail");
    source
        .write_str("fn add_pair(x: Int, y: Int) -> Int = x + y\n")
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.add_pair",
            "20",
            "22",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.add_pair"))
        .stdout(predicate::str::contains("result: 42"));
}
#[test]
fn run_file_executes_ail_source_block_with_let_statement() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("block.ail");
    source
        .write_str("fn main() -> Int {\n  let base = 20 + 20\n  return add(base, 2)\n}\n")
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.main"))
        .stdout(predicate::str::contains("result: 42"));
}
#[test]
fn run_file_executes_ail_source_if_else_expression() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("control.ail");
    source
        .write_str("fn clamp_positive(x: Int) -> Int {\n  if gt(x, 0) { x } else { 0 }\n}\n")
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.clamp_positive",
            "-5",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.clamp_positive"))
        .stdout(predicate::str::contains("result: 0"));
}
#[test]
fn run_file_executes_ail_source_text_concat_with_comment_markers_in_strings() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text.ail");
    source
        .write_str(
            "fn greeting() -> Text = concat(\"Hello, //\", \" world!\") // trailing comment\n",
        )
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.greeting",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.greeting"))
        .stdout(predicate::str::contains("result: Hello, // world!"));
}
#[test]
fn run_file_executes_ail_source_text_concat_operator() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_concat_operator.ail");
    source
        .write_str("fn greeting() -> Text = \"Hello, \" ++ \"AIL\"\n")
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.greeting",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.greeting"))
        .stdout(predicate::str::contains("result: Hello, AIL"));
}
#[test]
fn run_file_executes_ail_source_text_eq_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_eq.ail");
    source
        .write_str("fn same() -> Bool = text_eq(\"Hello, \" ++ \"AIL\", \"Hello, AIL\")\n")
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.same",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.same"))
        .stdout(predicate::str::contains("result: 1"));
}
#[test]
fn run_file_executes_ail_source_text_trim_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_trim.ail");
    source
        .write_str("fn cleaned() -> Text = text_trim(\"  AIL  \")\n")
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.cleaned",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.cleaned"))
        .stdout(predicate::str::contains("result: AIL"));
}

#[test]
fn run_file_executes_ail_source_path_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("path_helpers.ail");
    source
        .write_str(
            "fn config_path() -> Path = path_from_text(\"config/app.toml\")\n\
fn config_path_text() -> Text = path_to_text(path_from_text(\"config/app.toml\"))\n",
        )
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.config_path",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.config_path"))
        .stdout(predicate::str::contains("result: config/app.toml"));

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.config_path_text",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.config_path_text"))
        .stdout(predicate::str::contains("result: config/app.toml"));
}

#[test]
fn run_file_executes_ail_source_fs_read_file_bytes_with_grant() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let data = dir.child("data.bin");
    data.write_binary(b"AIL!")
        .expect("data fixture must be written");
    let source = dir.child("file_read.ail");
    source
        .write_str(
            "capability file.read\n\
fn main() -> Bytes = fs_read_file(path_from_text(\"data.bin\"))\n\
grant main file.read\n",
        )
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "--grant",
            "file.read",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.main"))
        .stdout(predicate::str::contains("result: bytes[41 49 4c 21]"));
}

#[test]
fn run_file_executes_ail_source_random_next_int_with_grant() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("random_next_int.ail");
    source
        .write_str(
            "capability random.int
fn main() -> Int = random_next_int()
grant main random.int
",
        )
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "--grant",
            "random.int",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.main"))
        .stdout(predicate::str::contains("result: 1082269761"));
}

#[test]
fn run_file_executes_ail_source_time_now_with_clock_grant() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("time_now.ail");
    source
        .write_str(
            "capability clock.now\n\
fn main() -> Int = time_now()\n\
grant main clock.now\n",
        )
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "--grant",
            "clock.now",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.main"))
        .stdout(
            predicate::str::is_match(r"result: [1-9][0-9]{12}")
                .expect("epoch-ms result regex must compile"),
        );
}

#[test]
fn run_file_executes_ail_source_length_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("length_helpers.ail");
    source
        .write_str("fn size() -> Int = add(text_length(\"AIL\"), list_length([1, 2, 3]))\n")
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.size",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.size"))
        .stdout(predicate::str::contains("result: 6"));
}

#[test]
fn run_file_executes_ail_source_text_byte_at_or_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_byte_at_or.ail");
    source
        .write_str("fn byte() -> Int = text_byte_at_or(\"AIL\", 1, -1)\n")
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.byte",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.byte"))
        .stdout(predicate::str::contains("result: 73"));
}
#[test]
fn run_file_executes_ail_source_int_bounds_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("int_bounds.ail");
    source
        .write_str(
            "fn bounded() -> Int = int_min(10, -2) + int_max(10, -2) + int_clamp(42, 0, 10) + int_abs_or(-7, 0) + int_abs_or(-9223372036854775808, 99) + int_neg_or(-5, 0) + int_neg_or(-9223372036854775808, 17) + int_add_or(40, 2, -1) + int_add_or(9223372036854775807, 1, 19) + int_sub_or(50, 8, -1) + int_sub_or(-9223372036854775808, 1, 23) + int_mul_or(6, 7, -1) + int_mul_or(9223372036854775807, 2, 29) + int_mul_or(-9223372036854775808, -1, 31) + int_saturating_add(40, 2) + int_saturating_sub(50, 8) + int_saturating_sub(-40, -2) + int_saturating_mul(6, 7) + int_saturating_mul(-6, 7) + int_saturating_neg(-5) + int_saturating_neg(5) + int_saturating_neg(-9223372036854775808) + int_saturating_neg(9223372036854775807) + int_wrapping_add(40, 2) + int_wrapping_add(9223372036854775807, 1) + int_wrapping_add(-9223372036854775808, -1) + int_wrapping_sub(50, 8) + int_wrapping_sub(-40, -2) + int_wrapping_mul(6, 7) + int_wrapping_mul(9223372036854775807, 2) + int_wrapping_neg(-5) + int_wrapping_neg(5) + int_bit_and(6, 3) + int_bit_and(-1, 42) + int_bit_or(4, 1) + int_bit_or(8, 3) + int_bit_xor(6, 3) + int_bit_xor(-1, 42) + int_bit_not(0) + int_bit_not(-1) + int_shift_left(1, 3) + int_shift_left(-1, 1) + int_shift_right(16, 1) + int_shift_right(-8, 1) + int_shift_right_unsigned(16, 1) + int_div_or(21, 3, -1) + int_div_or(1, 0, 5) + int_div_or(-9223372036854775808, -1, 11) + int_rem_or(22, 5, -1) + int_rem_or(1, 0, 6) + int_rem_or(-9223372036854775808, -1, 13)\n",
        )
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.bounded",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.bounded"))
        .stdout(predicate::str::contains("result: 584"));
}
#[test]
fn run_file_executes_ail_source_text_parse_int_or_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_parse_int_or.ail");
    source
        .write_str("fn parsed() -> Int = text_parse_int_or(\"-42\", 0)\n")
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.parsed",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.parsed"))
        .stdout(predicate::str::contains("result: -42"));
}
#[test]
fn run_file_executes_ail_source_text_parse_int_or_overflow_fallback() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_parse_int_or_overflow.ail");
    source
        .write_str("fn parsed() -> Int = text_parse_int_or(\"9223372036854775808\", -1)\n")
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.parsed",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.parsed"))
        .stdout(predicate::str::contains("result: -1"));
}
#[test]
fn run_file_executes_ail_source_text_contains_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_contains.ail");
    source
        .write_str("fn has() -> Bool = text_contains(\"Hello, \" ++ \"AIL\", \"lo, A\")\n")
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.has",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.has"))
        .stdout(predicate::str::contains("result: 1"));
}
#[test]
fn run_file_executes_ail_source_text_index_of_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_index_of.ail");
    source
        .write_str("fn idx() -> Int = text_index_of(\"Hello, \" ++ \"AIL\", \"AIL\")\n")
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.idx",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.idx"))
        .stdout(predicate::str::contains("result: 7"));
}
#[test]
fn run_file_executes_ail_source_text_slice_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_slice.ail");
    source
        .write_str("fn piece() -> Text = text_slice(\"Hello, \" ++ \"AIL\", 7, 3)\n")
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.piece",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.piece"))
        .stdout(predicate::str::contains("result: AIL"));
}
#[test]
fn run_file_executes_ail_source_text_replace_first_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_replace_first.ail");
    source
        .write_str(
            "fn changed() -> Text = text_replace_first(\"Hello, \" ++ \"AIL\", \"AIL\", \"World\")\n",
        )
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.changed",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.changed"))
        .stdout(predicate::str::contains("result: Hello, World"));
}
#[test]
fn run_file_executes_ail_source_text_boundary_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_boundary.ail");
    source
        .write_str(
            "fn ok() -> Bool = text_starts_with(\"Hello, \" ++ \"AIL\", \"Hell\") && text_ends_with(\"Hello, \" ++ \"AIL\", \"AIL\")\n",
        )
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.ok",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.ok"))
        .stdout(predicate::str::contains("result: 1"));
}
#[test]
fn run_file_executes_ail_source_with_capability_grant() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("print.ail");
    source
        .write_str(
            r#"capability log.write
fn print_hello() -> Int = print("Hello from source!")
grant print_hello log.write
"#,
        )
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "--grant",
            "log.write",
            "fn.print_hello",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("source:"))
        .stdout(predicate::str::contains("module: fn.print_hello"))
        .stdout(predicate::str::contains("output:\nHello from source!"))
        .stdout(predicate::str::contains("result: 0"));
}
#[test]
fn run_file_executes_explicit_ail_source_effect_call() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("effect.ail");
    source
        .write_str(
            r#"capability log.write
fn emit() -> Int = effect_call(log.write, write, "Hello from effect_call!")
grant emit log.write
"#,
        )
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "--grant",
            "log.write",
            "fn.emit",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.emit"))
        .stdout(predicate::str::contains("output:\nHello from effect_call!"))
        .stdout(predicate::str::contains("result: 0"));
}
#[test]
fn run_file_uses_source_module_main_entry_by_default() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("module app\nfn main() -> Int = add(20, 22)\n")
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("source:"))
        .stdout(predicate::str::contains("module: fn.app.main"))
        .stdout(predicate::str::contains("result: 42"));
}
#[test]
fn run_file_rejects_missing_source_entrypoint() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("lib.ail");
    source
        .write_str("module app\nfn helper() -> Int = 42\n")
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
        ])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "source entrypoint `fn.app.main` was not exported as `app_main`",
        ));
}
#[test]
fn run_file_executes_ail_source_imports_relative_files() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let math = dir.child("math.ail");
    math.write_str("module math\nfn add_pair(x: Int, y: Int) -> Int = x + y\n")
        .expect("imported source fixture must be written");
    let main = dir.child("main.ail");
    main.write_str("use \"./math.ail\"\nfn main() -> Int = math.add_pair(20, 22)\n")
        .expect("main source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            main.path().to_str().expect("path must be UTF-8"),
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("source:"))
        .stdout(predicate::str::contains("module: fn.main"))
        .stdout(predicate::str::contains("result: 42"));
}
#[test]
fn run_text_return_prints_human_readable_result() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    let acl = dir.child("hello-text.acl");
    acl.write_str(
        r#"change hello_text
author cli-test
description text return hello world
base 0
op create_function id=fn.hello return=Text body=let(s, "Hello, world!", s)
end
"#,
    )
    .expect("ACL fixture must be written");

    let change_output = ail()
        .args([
            "change",
            "--file",
            acl.path().to_str().expect("path must be UTF-8"),
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let change_json = parse_json_output(&change_output);
    let change_id = change_json["data"]["change_id"]
        .as_str()
        .or_else(|| change_json["data"]["canonical_change"]["change_id"].as_str())
        .expect("change output must include a change_id");

    ail()
        .args(["verify", change_id])
        .current_dir(dir.path())
        .assert()
        .success();
    ail()
        .args(["apply", change_id, "--yes"])
        .current_dir(dir.path())
        .assert()
        .success();
    ail()
        .args(["compile", "--profile", "dev", "--target", "wasm"])
        .current_dir(dir.path())
        .assert()
        .success();

    ail()
        .args(["run", "--profile", "dev", "--target", "wasm", "fn.hello"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("result: Hello, world!"));

    let run_output = ail()
        .args([
            "run",
            "--profile",
            "dev",
            "--target",
            "wasm",
            "--json",
            "fn.hello",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let run_json = parse_json_output(&run_output);
    assert_eq!(run_json["data"]["invoke_result"], "result: Hello, world!");
    assert_eq!(run_json["data"]["invoke_value"], "Hello, world!");
}
#[test]
fn run_file_prints_structured_collection_results() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("structured.ail");
    source
        .write_str(
            "fn numbers() -> List<Int> = [1, 2, 3]\n\
fn names() -> List<Text> = [\"Ada\", \"AIL\"]\n\
fn pair() -> Tuple<Int, Int> = tuple(42, 7)\n\
fn person() -> Record<age:Int,score:Int> = { age: 42, score: 7 }\n",
        )
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.numbers",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.numbers"))
        .stdout(predicate::str::contains("result: [1, 2, 3]"));

    let names_output = ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "--json",
            "fn.names",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let names_json = parse_json_output(&names_output);
    assert_eq!(names_json["data"]["invoke_result"], "result: [Ada, AIL]");
    assert_eq!(
        names_json["data"]["invoke_value"],
        serde_json::json!(["Ada", "AIL"])
    );

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.pair",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.pair"))
        .stdout(predicate::str::contains("result: (42, 7)"));

    let output = ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "--json",
            "fn.person",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let json = parse_json_output(&output);
    assert_eq!(json["data"]["invoke_result"], "result: {age: 42, score: 7}");
    assert_eq!(json["data"]["invoke_value"]["age"], 42);
    assert_eq!(json["data"]["invoke_value"]["score"], 7);
}

#[test]
fn run_file_prints_declared_option_and_result_values() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("variants.ail");
    source
        .write_str(
            "fn maybe() -> Option<Int> = Some(42)\n\
fn missing() -> Option<Int> = None\n\
fn outcome() -> Result<Int, Text> = Ok(7)\n\
fn failure() -> Result<Int, Text> = Err(\"bad\")\n",
        )
        .expect("source fixture must be written");

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.maybe",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.maybe"))
        .stdout(predicate::str::contains("result: Some(42)"));

    ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "fn.missing",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("module: fn.missing"))
        .stdout(predicate::str::contains("result: None"));

    let ok_output = ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "--json",
            "fn.outcome",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let ok_json = parse_json_output(&ok_output);
    assert_eq!(ok_json["data"]["invoke_result"], "result: Ok(7)");
    assert_eq!(ok_json["data"]["invoke_value"]["tag"], "Ok");
    assert_eq!(ok_json["data"]["invoke_value"]["value"], 7);
    assert_eq!(
        ok_json["data"]["invoke_abi_descriptor_source"],
        "source_declared_return"
    );
    assert_eq!(ok_json["data"]["invoke_abi_descriptor"]["abi_version"], 1);
    assert_eq!(
        ok_json["data"]["invoke_abi_descriptor"]["exports"]["outcome"],
        serde_json::json!({ "Result": { "ok": { "Scalar": "I64" }, "err": "Text" } })
    );
    assert_eq!(
        ok_json["data"]["runtime_check_results"]["abi_descriptor"]["passed"],
        true
    );

    let err_output = ail()
        .args([
            "run",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "--json",
            "fn.failure",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let err_json = parse_json_output(&err_output);
    assert_eq!(err_json["data"]["invoke_result"], "result: Err(bad)");
    assert_eq!(err_json["data"]["invoke_value"]["tag"], "Err");
    assert_eq!(err_json["data"]["invoke_value"]["value"], "bad");
}

#[test]
fn run_print_requires_log_write_grant_and_captures_output() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    let acl = dir.child("print-hello.acl");
    acl.write_str(
        r#"change print_hello
author cli-test
description print hello world
base 0
op create_capability id=log.write
op create_function id=fn.print_hello return=Int body=print("Hello, world!")
op grant target=fn.print_hello capability=log.write
end
"#,
    )
    .expect("ACL fixture must be written");

    let change_output = ail()
        .args([
            "change",
            "--file",
            acl.path().to_str().expect("path must be UTF-8"),
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let change_json = parse_json_output(&change_output);
    let change_id = change_json["data"]["canonical_change"]["change_id"]
        .as_str()
        .expect("change output must include a change_id");

    ail()
        .args(["verify", change_id])
        .current_dir(dir.path())
        .assert()
        .success();
    ail()
        .args(["apply", change_id, "--yes"])
        .current_dir(dir.path())
        .assert()
        .success();
    ail()
        .args(["compile", "--profile", "dev", "--target", "wasm"])
        .current_dir(dir.path())
        .assert()
        .success();

    ail()
        .args([
            "run",
            "--profile",
            "dev",
            "--target",
            "wasm",
            "fn.print_hello",
        ])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("function `fn.print_hello`"))
        .stderr(predicate::str::contains("capability `log.write`"))
        .stderr(predicate::str::contains(
            "suggestion: add `--grant log.write`",
        ));

    ail()
        .args([
            "run",
            "--profile",
            "dev",
            "--target",
            "wasm",
            "--grant",
            "log.write",
            "fn.print_hello",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("output:\nHello, world!"))
        .stdout(predicate::str::contains("result: 0"));

    let run_output = ail()
        .args([
            "run",
            "--profile",
            "dev",
            "--target",
            "wasm",
            "--grant",
            "log.write",
            "--json",
            "fn.print_hello",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let run_json = parse_json_output(&run_output);
    assert_eq!(run_json["data"]["invoke_result"], "result: 0");
    assert_eq!(
        run_json["data"]["output"],
        serde_json::json!(["Hello, world!"])
    );
}
#[test]
fn run_print_without_graph_grant_fails_preflight() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    let acl = dir.child("print-without-grant.acl");
    acl.write_str(
        r#"change print_without_grant
author cli-test
description print without graph grant
base 0
op create_capability id=log.write
op create_function id=fn.print_without_grant return=Int body=print("Hello, world!")
end
"#,
    )
    .expect("ACL fixture must be written");

    let change_output = ail()
        .args([
            "change",
            "--file",
            acl.path().to_str().expect("path must be UTF-8"),
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let change_json = parse_json_output(&change_output);
    let change_id = change_json["data"]["canonical_change"]["change_id"]
        .as_str()
        .expect("change output must include a change_id");

    ail()
        .args(["verify", change_id])
        .current_dir(dir.path())
        .assert()
        .success();
    ail()
        .args(["apply", change_id, "--yes"])
        .current_dir(dir.path())
        .assert()
        .success();
    ail()
        .args(["compile", "--profile", "dev", "--target", "wasm"])
        .current_dir(dir.path())
        .assert()
        .success();

    ail()
        .args([
            "run",
            "--profile",
            "dev",
            "--target",
            "wasm",
            "fn.print_without_grant",
        ])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "function `fn.print_without_grant`",
        ))
        .stderr(predicate::str::contains("capability `log.write`"))
        .stderr(predicate::str::contains(
            "suggestion: add `--grant log.write`",
        ));
}
#[test]
fn run_print_transitive_callee_requires_log_write_grant() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    let acl = dir.child("transitive-print.acl");
    acl.write_str(
        r#"change transitive_print
author cli-test
description transitive print requires log write
base 0
op create_capability id=log.write
op create_function id=fn.print_hello return=Int body=print("Hello from callee!")
op grant target=fn.print_hello capability=log.write
op create_function id=fn.main return=Int body=print_hello()
end
"#,
    )
    .expect("ACL fixture must be written");

    let change_output = ail()
        .args([
            "change",
            "--file",
            acl.path().to_str().expect("path must be UTF-8"),
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let change_json = parse_json_output(&change_output);
    let change_id = change_json["data"]["canonical_change"]["change_id"]
        .as_str()
        .expect("change output must include a change_id");

    ail()
        .args(["verify", change_id])
        .current_dir(dir.path())
        .assert()
        .success();
    ail()
        .args(["apply", change_id, "--yes"])
        .current_dir(dir.path())
        .assert()
        .success();
    ail()
        .args(["compile", "--profile", "dev", "--target", "wasm"])
        .current_dir(dir.path())
        .assert()
        .success();

    ail()
        .args(["run", "--profile", "dev", "--target", "wasm", "fn.main"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("function `fn.main`"))
        .stderr(predicate::str::contains("capability `log.write`"))
        .stderr(predicate::str::contains(
            "suggestion: add `--grant log.write`",
        ));

    ail()
        .args([
            "run",
            "--profile",
            "dev",
            "--target",
            "wasm",
            "--grant",
            "log.write",
            "fn.main",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("output:\nHello from callee!"))
        .stdout(predicate::str::contains("result: 0"));
}
