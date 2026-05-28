// ── ail-cli integration tests: subcommands (G8 + G31) ─────────────────────
//
// Covers:
//   G8  — init, status, inspect, diff subcommands
//   G31 — rollback, rebase, merge, refactor, approve, reject, policy, doctor
//
// Shared helpers live in common/mod.rs.

mod common;

use common::{ail, parse_json_output, sample_acl_path};
use predicates::prelude::*;

// ── G8: new subcommands (init, status, inspect, diff) ─────────────────────

/// Spec scenario: init exits 0 and creates .ail/ directory.
///   GIVEN an empty temp directory
///   WHEN `ail init` runs
///   THEN exit 0; .ail/ dir created; JSON reports initialized=true
#[test]
fn init_exits_zero_and_creates_ail_dir() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let output = ail()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = std::str::from_utf8(&output.stdout).expect("stdout must be UTF-8");
    assert!(
        stdout.contains("initialized") || stdout.contains(".ail"),
        "init output must mention initialization; got:\n{stdout}"
    );

    // .ail/ must have been created.
    dir.child(".ail").assert(predicate::path::is_dir());
}

/// Spec scenario: init is idempotent.
///   GIVEN `ail init` has already been run
///   WHEN `ail init` runs again
///   THEN exit 0; no error
#[test]
fn init_is_idempotent() {
    use assert_fs::prelude::*;
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    // First init.
    ail().arg("init").current_dir(dir.path()).assert().success();
    // Second init must also succeed.
    ail().arg("init").current_dir(dir.path()).assert().success();
    dir.child(".ail").assert(predicate::path::is_dir());
}

/// Spec scenario: init JSON output contains genesis_snapshot_id.
///   GIVEN `ail init --json` runs
///   WHEN init completes
///   THEN JSON has initialized=true and genesis_snapshot_id string
#[test]
fn init_json_output_has_genesis_id() {
    use assert_fs::TempDir;
    let dir = TempDir::new().expect("temp dir");
    let output = ail()
        .args(["init", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["initialized"], true);
    assert!(
        v["data"]["genesis_snapshot_id"].is_string(),
        "genesis_snapshot_id must be a string; got: {v}"
    );
}

/// Spec scenario: new creates a project scaffold.
///   GIVEN an empty parent directory
///   WHEN `ail new myapp --json` runs
///   THEN myapp has .ail metadata, starter .ail source, starter ACL, and JSON scaffold fields
#[test]
fn new_creates_project_scaffold_with_starter_source_and_acl() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let output = ail()
        .args(["new", "myapp", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let project = dir.child("myapp");
    project.child(".ail/HEAD").assert(predicate::path::exists());
    project
        .child(".ail/store/objects")
        .assert(predicate::path::is_dir());
    project.child("main.ail").assert(predicate::path::exists());
    project.child("main.acl").assert(predicate::path::exists());

    let source = std::fs::read_to_string(project.path().join("main.ail"))
        .expect("starter source must be readable");
    assert!(source.contains("fn main() -> Int"));
    assert!(source.contains("test main_addition"));

    let acl = std::fs::read_to_string(project.path().join("main.acl"))
        .expect("starter ACL must be readable");
    assert!(acl.contains("op create_function id=fn.main"));
    assert!(acl.contains("op create_test id=test.main_addition"));

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["created"], true);
    assert_eq!(v["data"]["branch"], "main");
    assert!(v["data"]["genesis_snapshot_id"].is_string());
    assert!(v["data"]["starter_source"].is_string());
    assert!(v["data"]["starter_acl"].is_string());
}

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
        .write_str("fn add_pair(x: Int, y: Int) -> Int = add(x, y)\n")
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
        .write_str("fn main() -> Int {\n  let base = add(20, 20)\n  return add(base, 2)\n}\n")
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
    math.write_str("module math\nfn add_pair(x: Int, y: Int) -> Int = add(x, y)\n")
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
fn test_file_runs_ail_source_tests_without_acl_authoring() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn main() -> Int = add(20, 22)\n\
fn add_pair(x: Int, y: Int) -> Int = add(x, y)\n\
test main_addition = eq(add_pair(20, 22), 42)\n",
        )
        .expect("source fixture must be written");

    ail()
        .args([
            "test",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("PASS test.main_addition"))
        .stdout(predicate::str::contains("test result: 1 passed; 0 failed"));
}

#[test]
fn test_file_runs_module_qualified_ail_source_tests() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "module app\n\
fn main() -> Int = add(20, 22)\n\
test main_addition = eq(main(), 42)\n",
        )
        .expect("source fixture must be written");

    let output = ail()
        .args([
            "test",
            "--file",
            source.path().to_str().expect("path must be UTF-8"),
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["passed"], 1);
    assert_eq!(v["data"]["failed"], 0);
    assert_eq!(v["data"]["tests"][0]["name"], "test.app.main_addition");
    assert_eq!(v["data"]["tests"][0]["export"], "app_main_addition");
}

#[test]
fn check_file_validates_ail_source_without_execution() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let math = dir.child("math.ail");
    math.write_str("module math\nfn add_pair(x: Int, y: Int) -> Int = add(x, y)\n")
        .expect("imported source fixture must be written");
    let source = dir.child("main.ail");
    source
        .write_str("use \"./math.ail\"\nfn main() -> Int = math.add_pair(20, 22)\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["check", "--file"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["functions"], 2);
    assert_eq!(v["data"]["imports"], 1);
    assert_eq!(v["data"]["default_entry"], "fn.main");
    assert!(
        v["data"]["graph_nodes"].as_u64().unwrap() >= 2,
        "check must materialize source into a semantic graph"
    );
    assert!(
        v["data"]["graph_edges"].as_u64().is_some(),
        "check must report graph edge materialization"
    );
}

#[test]
fn check_file_rejects_invalid_ail_source_without_execution() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad.ail");
    source
        .write_str("fn main() -> Int = add(x, 1)\n")
        .expect("source fixture must be written");

    ail()
        .args(["check", "--file"])
        .arg(source.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown variable `x`"));
}

#[test]
fn compile_file_compiles_ail_source_without_acl_authoring() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let math = dir.child("math.ail");
    math.write_str("fn add_pair(x: Int, y: Int) -> Int = add(x, y)\n")
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
            "type mismatch in fn.main: expected Int, got Float",
        ));
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
fn compile_file_rejects_unsupported_source_expression_syntax() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_expr.ail");
    source
        .write_str("fn main() -> Int = 1 + 2\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unsupported source expression `1 + 2`",
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
    let source = dir.child("bad_list.ail");
    source
        .write_str("fn main() -> Int = list(1, 2)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unsupported source builtin `list` has no type inference",
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
fn compile_file_accepts_module_test_capability_grants() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("module_test_grant.ail");
    source
        .write_str(
            "module app
capability log.write
test smoke -> Int = effect_call(log.write, write, \"hi\")
grant smoke log.write
fn main() -> Int = 0
",
        )
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .args(["--profile", "dev", "--target", "wasm", "--json"])
        .current_dir(dir.path())
        .assert()
        .success();
}

#[test]
fn compile_file_rejects_ambiguous_source_grants() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("ambiguous_grant.ail");
    source
        .write_str(
            "capability log.write
fn smoke() -> Int = 0
test smoke -> Int = effect_call(log.write, write, \"hi\")
grant smoke log.write
fn main() -> Int = 0
",
        )
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "grant target `smoke` is ambiguous; use `fn.smoke` or `test.smoke`",
        ));
}

#[test]
fn compile_file_accepts_explicit_test_source_grants() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("explicit_test_grant.ail");
    source
        .write_str(
            "capability log.write
fn smoke() -> Int = 0
test smoke -> Int = effect_call(log.write, write, \"hi\")
grant test.smoke log.write
fn main() -> Int = 0
",
        )
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .args(["--profile", "dev", "--target", "wasm", "--json"])
        .current_dir(dir.path())
        .assert()
        .success();
}

#[test]
fn compile_file_rejects_ungranted_source_effect_call() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("effect_no_grant.ail");
    source
        .write_str(
            "capability log.write\nfn main() -> Int = effect_call(log.write, write, \"hi\")\n",
        )
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "source item `fn.main` uses capability `log.write` without a grant",
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

#[test]
fn compile_file_rejects_bare_source_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_bare_import.ail");
    source
        .write_str("use \"math.ail\"\nfn main() -> Int = 0\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "local import path `math.ail` must start with `./`",
        ));
}

#[test]
fn compile_file_rejects_whitespace_source_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_whitespace_import.ail");
    source
        .write_str("use \"./my lib.ail\"\nfn main() -> Int = 0\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "import path `./my lib.ail` must not contain whitespace",
        ));
}

#[test]
fn compile_file_rejects_empty_segment_source_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_empty_segment_import.ail");
    source
        .write_str("use \"./math//util.ail\"\nfn main() -> Int = 0\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "import path `./math//util.ail` must not contain empty path segments",
        ));
}

#[test]
fn compile_file_rejects_colon_source_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_colon_import.ail");
    source
        .write_str("use \"C:/math.ail\"\nfn main() -> Int = 0\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "import path `C:/math.ail` must not contain `:`",
        ));
}

#[test]
fn compile_file_rejects_backslash_source_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_backslash_import.ail");
    source
        .write_str("use \".\\math.ail\"\nfn main() -> Int = 0\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "import path `.\\math.ail` must use `/` separators",
        ));
}

#[test]
fn compile_file_rejects_parent_source_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_parent_import.ail");
    source
        .write_str("use \"../math.ail\"\nfn main() -> Int = 0\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "import path `../math.ail` must not contain `..`",
        ));
}

#[test]
fn compile_file_rejects_non_ail_source_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_import.ail");
    source
        .write_str("use \"./math.txt\"\nfn main() -> Int = 0\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "import path `./math.txt` must end with `.ail`",
        ));
}

#[test]
fn compile_file_rejects_invalid_ail_source_before_lowering() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad.ail");
    source
        .write_str("fn main() -> Int = add(\"one\", 1)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("type mismatch in add argument 1"));
}

/// Spec scenario: lsp diagnose emits parser/schema diagnostics.
///   GIVEN an ACL file whose create_function op is missing id
///   WHEN `ail lsp --diagnose <file> --json` runs
///   THEN JSON contains an LSP-shaped diagnostic with source ail-acl-schema
#[test]
fn lsp_diagnose_reports_acl_schema_errors() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let acl = dir.child("bad.acl");
    acl.write_str(
        r#"change bad
author cli-test
description bad ACL
base 0
op create_function return=Int
end
"#,
    )
    .expect("ACL fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(acl.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["source"], "ail-acl-schema");
    assert_eq!(v["data"]["diagnostics"][0]["severity"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("requires argument 'id'")
    );
}

#[test]
fn lsp_diagnose_reports_ail_source_parse_errors() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad.ail");
    source
        .write_str("fn broken(x Int) -> Int = x\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["source"], "ail-source-parser");
    assert_eq!(v["data"]["diagnostics"][0]["severity"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("function parameters must use `name: Type`")
    );
}

#[test]
fn lsp_diagnose_reports_bare_source_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("use \"math.ail\"\nfn main() -> Int = 0\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["source"], "ail-source-parser");
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("local import path `math.ail` must start with `./`")
    );
}

#[test]
fn lsp_diagnose_reports_whitespace_source_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("use \"./my lib.ail\"\nfn main() -> Int = 0\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["source"], "ail-source-parser");
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("import path `./my lib.ail` must not contain whitespace")
    );
}

#[test]
fn lsp_diagnose_reports_empty_segment_source_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("use \"./math//util.ail\"\nfn main() -> Int = 0\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["source"], "ail-source-parser");
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("import path `./math//util.ail` must not contain empty path segments")
    );
}

#[test]
fn lsp_diagnose_reports_colon_source_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("use \"C:/math.ail\"\nfn main() -> Int = 0\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["source"], "ail-source-parser");
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("import path `C:/math.ail` must not contain `:`")
    );
}

#[test]
fn lsp_diagnose_reports_backslash_source_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("use \".\\math.ail\"\nfn main() -> Int = 0\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["source"], "ail-source-parser");
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("import path `.\\math.ail` must use `/` separators")
    );
}

#[test]
fn lsp_diagnose_reports_parent_source_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("use \"../math.ail\"\nfn main() -> Int = 0\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["source"], "ail-source-parser");
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("import path `../math.ail` must not contain `..`")
    );
}

#[test]
fn lsp_diagnose_reports_non_ail_source_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("use \"./math.txt\"\nfn main() -> Int = 0\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["source"], "ail-source-parser");
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("import path `./math.txt` must end with `.ail`")
    );
}

#[test]
fn lsp_diagnose_reports_ail_source_missing_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("use \"./missing.ail\"\nfn main() -> Int = 0\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["source"], "ail-source-import");
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("failed to resolve AIL source")
    );
}

#[test]
fn lsp_diagnose_reports_ail_source_cyclic_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let main = dir.child("main.ail");
    main.write_str("use \"./dep.ail\"\nfn main() -> Int = dep()\n")
        .expect("main fixture must be written");
    let dep = dir.child("dep.ail");
    dep.write_str("use \"./main.ail\"\nfn dep() -> Int = 42\n")
        .expect("dep fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(main.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert_eq!(v["data"]["diagnostics"][0]["source"], "ail-source-import");
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("cyclic AIL source import detected")
    );
}

#[test]
fn lsp_diagnose_reports_ail_source_duplicate_imported_functions() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let dep = dir.child("dep.ail");
    dep.write_str("fn helper() -> Int = 1\n")
        .expect("dep fixture must be written");
    let main = dir.child("main.ail");
    main.write_str("use \"./dep.ail\"\nfn helper() -> Int = 2\n")
        .expect("main fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(main.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("duplicate function declaration `fn.helper`")
    );
}

#[test]
fn lsp_diagnose_reports_ail_source_unknown_grant_capability() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = 0\ngrant main log.write\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("grant capability `log.write` is not declared")
    );
}

#[test]
fn lsp_diagnose_accepts_module_test_capability_grants() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "module app
capability log.write
test smoke -> Int = effect_call(log.write, write, \"hi\")
grant smoke log.write
fn main() -> Int = 0
",
        )
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 0);
    assert_eq!(v["data"]["error_count"], 0);
}

#[test]
fn lsp_diagnose_reports_ambiguous_source_grants() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "capability log.write
fn smoke() -> Int = 0
test smoke -> Int = effect_call(log.write, write, \"hi\")
grant smoke log.write
fn main() -> Int = 0
",
        )
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("grant target `smoke` is ambiguous; use `fn.smoke` or `test.smoke`")
    );
}

#[test]
fn lsp_diagnose_reports_ungranted_source_effect_call() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "capability log.write\nfn main() -> Int = effect_call(log.write, write, \"hi\")\n",
        )
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("source item `fn.main` uses capability `log.write` without a grant")
    );
}

#[test]
fn lsp_diagnose_reports_ail_source_unknown_effect_call_capability() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = effect_call(log.write, write, \"hi\")\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("effect_call capability `log.write` is not declared")
    );
}

#[test]
fn lsp_diagnose_reports_ail_source_unknown_grant_target() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("capability log.write\ngrant missing log.write\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("grant target `fn.missing` is not declared")
    );
}

#[test]
fn lsp_diagnose_reports_ail_source_unknown_function_calls() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = typo_add(20, 22)\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unknown function call `typo_add`")
    );
}

#[test]
fn lsp_diagnose_reports_ail_source_builtin_call_arity_errors() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = add(20)\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("function call `add` expects 2 argument(s), got 1")
    );
}

#[test]
fn lsp_diagnose_reports_ail_source_user_call_arity_errors() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn add_pair(x: Int, y: Int) -> Int = add(x, y)\nfn main() -> Int = add_pair(20)\n",
        )
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("function call `add_pair` expects 2 argument(s), got 1")
    );
}

#[test]
fn lsp_diagnose_reports_ail_source_unknown_variables() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = add(x, 1)\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unknown variable `x`")
    );
}

#[test]
fn lsp_diagnose_accepts_ail_source_params_and_let_variables() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn plus_one(x: Int) -> Int {\n  let base = add(x, 1)\n  return add(base, 1)\n}\n",
        )
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 0);
    assert_eq!(v["data"]["error_count"], 0);
}

#[test]
fn lsp_diagnose_reports_ail_source_return_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = true\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("type mismatch in fn.main: expected Int, got Bool")
    );
}

#[test]
fn lsp_diagnose_reports_ail_source_float_literal_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = 1.5\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("type mismatch in fn.main: expected Int, got Float")
    );
}

#[test]
fn lsp_diagnose_reports_numeric_leading_source_names() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn 1bad() -> Int = 1\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("declaration name `1bad` segment `1bad` must start with a letter or `_`")
    );
}

#[test]
fn lsp_diagnose_reports_malformed_source_names() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn bad..name() -> Int = 1\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("declaration name `bad..name` contains an empty path segment")
    );
}

#[test]
fn lsp_diagnose_reports_unsupported_source_parameter_type() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main(x: Mystery) -> Int = 1\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unsupported source type `Mystery`")
    );
}

#[test]
fn lsp_diagnose_reports_source_functions_that_shadow_builtins() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn add(x: Int) -> Int = x\nfn main() -> Int = add(1)\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("function declaration `fn.add` shadows builtin `add`")
    );
}

#[test]
fn lsp_diagnose_reports_duplicate_source_parameters() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main(x: Int, x: Int) -> Int = x\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("duplicate parameter `x`")
    );
}

#[test]
fn lsp_diagnose_reports_dotted_source_parameter_names() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main(x.y: Int) -> Int = x.y\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("local binding name `x.y` must not contain `.`")
    );
}

#[test]
fn lsp_diagnose_reports_dotted_source_let_names() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int {\n  let x.y = 1\n  return x.y\n}\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("local binding name `x.y` must not contain `.`")
    );
}

#[test]
fn lsp_diagnose_reports_non_finite_source_numeric_literals() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Float = NaN\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unsupported source numeric literal `NaN`")
    );
}

#[test]
fn lsp_diagnose_reports_unsupported_source_expression_syntax() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = 1 + 2\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unsupported source expression `1 + 2`")
    );
}

#[test]
fn lsp_diagnose_reports_unsupported_source_string_escapes() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Text = \"bad \\q escape\"\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("malformed string literal")
    );
}

#[test]
fn lsp_diagnose_reports_malformed_source_string_literals() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Text = \"unterminated\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("malformed string literal")
    );
}

#[test]
fn lsp_diagnose_reports_untyped_source_builtins() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = list(1, 2)\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("unsupported source builtin `list` has no type inference")
    );
}

#[test]
fn lsp_diagnose_reports_ail_source_builtin_argument_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = add(\"one\", 1)\n")
        .expect("source fixture must be written");

    let output = ail()
        .args(["lsp", "--diagnose"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["diagnostic_count"], 1);
    assert_eq!(v["data"]["error_count"], 1);
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("type mismatch in add argument 1: expected Int, got Text")
    );
}

#[test]
fn lsp_stdio_publish_diagnostics_reports_ail_source_missing_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = 0\n")
        .expect("source fixture must be written so file URI can resolve");
    let text = "use \"./missing.ail\"\nfn main() -> Int = 0\n";
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": format!("file://{}", source.path().display()),
                "languageId": "ail",
                "version": 1,
                "text": text,
            }
        }
    })
    .to_string();
    let input = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);

    ail()
        .args(["lsp", "--stdio"])
        .write_stdin(input)
        .assert()
        .success()
        .stdout(predicate::str::contains("textDocument/publishDiagnostics"))
        .stdout(predicate::str::contains("ail-source-import"))
        .stdout(predicate::str::contains("failed to resolve AIL source"));
}

#[test]
fn lsp_completion_and_hover_cover_acl_test_authoring() {
    let completion_output = ail()
        .args(["lsp", "--complete", "create_test", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let completion = parse_json_output(&completion_output);
    assert_eq!(completion["status"], "ok");
    let items = completion["data"]["items"]
        .as_array()
        .expect("completion items must be an array");
    assert!(
        items.iter().any(|item| item["label"] == "op create_test"
            && item["insertText"]
                .as_str()
                .expect("insertText")
                .contains("op create_test id=test.")),
        "completion must include create_test snippet; got: {items:?}"
    );

    let hover_output = ail()
        .args(["lsp", "--hover-token", "create_test", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let hover = parse_json_output(&hover_output);
    assert_eq!(hover["status"], "ok");
    assert!(
        hover["data"]["hover"]["contents"]["value"]
            .as_str()
            .expect("hover markdown")
            .contains("ail test"),
        "hover must explain create_test integration with ail test; got: {hover}"
    );
}

#[test]
fn lsp_definition_resolves_acl_target_to_id_location() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let acl = dir.child("defs.acl");
    acl.write_str(
        r#"change defs
author cli-test
description definition lookup
base 0
op create_function id=fn.main return=Int body=add(20, 22)
op grant target=fn.main capability=log.write
end
"#,
    )
    .expect("ACL fixture must be written");

    let output = ail()
        .args(["lsp", "--definition-token", "fn.main", "--definition-file"])
        .arg(acl.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();
    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["token"], "fn.main");
    assert_eq!(v["data"]["definition"]["range"]["start"]["line"], 4);
    assert!(
        v["data"]["definition"]["uri"]
            .as_str()
            .expect("definition uri")
            .ends_with("defs.acl")
    );
}

#[test]
fn lsp_definition_resolves_ail_source_imported_function() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let math = dir.child("math.ail");
    math.write_str("module math\nfn add_pair(x: Int, y: Int) -> Int = add(x, y)\n")
        .expect("imported source fixture must be written");
    let main = dir.child("main.ail");
    main.write_str("use \"./math.ail\"\nfn main() -> Int = math.add_pair(20, 22)\n")
        .expect("main source fixture must be written");

    let output = ail()
        .args([
            "lsp",
            "--definition-token",
            "math.add_pair",
            "--definition-file",
        ])
        .arg(main.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();
    let v = parse_json_output(&output);

    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["token"], "math.add_pair");
    assert_eq!(v["data"]["definition"]["range"]["start"]["line"], 1);
    assert_eq!(v["data"]["definition"]["range"]["start"]["character"], 3);
    assert!(
        v["data"]["definition"]["uri"]
            .as_str()
            .expect("definition uri")
            .ends_with("math.ail")
    );
}

#[test]
fn lsp_references_find_same_file_acl_identifier_uses() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let acl = dir.child("refs.acl");
    acl.write_str(
        r#"change refs
author cli-test
description reference lookup
base 0
op create_function id=fn.main return=Int body=add(20, 22)
op grant target=fn.main capability=log.write
op set_body target=fn.main body=add(1, 2)
end
"#,
    )
    .expect("ACL fixture must be written");

    let output = ail()
        .args(["lsp", "--references-token", "fn.main", "--references-file"])
        .arg(acl.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();
    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["token"], "fn.main");
    assert_eq!(v["data"]["reference_count"], 3);
    let refs = v["data"]["references"]
        .as_array()
        .expect("references must be an array");
    assert_eq!(refs[0]["range"]["start"]["line"], 4);
    assert_eq!(refs[1]["range"]["start"]["line"], 5);
    assert_eq!(refs[2]["range"]["start"]["line"], 6);
}

#[test]
fn lsp_references_resolve_ail_source_imported_function_uses() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let math = dir.child("math.ail");
    math.write_str("module math\nfn add_pair(x: Int, y: Int) -> Int = add(x, y)\n")
        .expect("imported source fixture must be written");
    let main = dir.child("main.ail");
    main.write_str(
        "use \"./math.ail\"\nfn main() -> Int = math.add_pair(20, 22)\ntest add = eq(math.add_pair(1, 2), 3)\n",
    )
    .expect("main source fixture must be written");

    let output = ail()
        .args([
            "lsp",
            "--references-token",
            "math.add_pair",
            "--references-file",
        ])
        .arg(main.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();
    let v = parse_json_output(&output);
    let refs = v["data"]["references"]
        .as_array()
        .expect("references must be an array");

    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["token"], "math.add_pair");
    assert_eq!(v["data"]["reference_count"], 3);
    assert_eq!(refs[0]["range"]["start"]["line"], 1);
    assert_eq!(refs[1]["range"]["start"]["line"], 2);
    assert!(
        refs[2]["uri"]
            .as_str()
            .expect("definition reference uri")
            .ends_with("math.ail")
    );
    assert_eq!(refs[2]["range"]["start"]["line"], 1);
}

/// Spec scenario: file-backed store persists between CLI invocations.
///   GIVEN `ail init` has created an on-disk store
///   WHEN `ail change` writes a snapshot and `ail compile` runs later
///   THEN compile loads the persisted graph and .ail layout exists
/// Spec scenario: file-backed store persists between CLI invocations.
///   GIVEN `ail init` has created an on-disk store
///   WHEN `ail change --file` creates a draft and `ail apply` applies it,
///   THEN compile loads the persisted graph and .ail layout exists.
///
/// Note: `ail change --file` defaults to draft mode (no snapshot).
/// `ail apply <change_id>` is required to create the snapshot.
#[test]
fn disk_store_persists_change_for_compile() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");

    ail().arg("init").current_dir(dir.path()).assert().success();

    // Step 1: create draft changeset.
    let change_output = ail()
        .args([
            "change",
            "--file",
            sample_acl_path().to_str().expect("path must be UTF-8"),
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let change_json = parse_json_output(&change_output);
    assert_eq!(
        change_json["data"]["status"], "draft",
        "change must default to draft"
    );
    let change_id = change_json["data"]["canonical_change"]["change_id"]
        .as_str()
        .expect("change_id must be present")
        .to_string();

    // Step 2: verify the draft from persisted .ail state.
    let verify_output = ail()
        .args(["verify", &change_id, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let verify_json = parse_json_output(&verify_output);
    assert_eq!(verify_json["status"], "ok");
    assert!(verify_json["data"]["verification_report"].is_object());

    // Step 3: apply the draft to create a snapshot.
    ail()
        .args(["apply", &change_id])
        .current_dir(dir.path())
        .assert()
        .success();

    let compile_output = ail()
        .args(["compile", "--profile", "dev", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let compile_json = parse_json_output(&compile_output);
    assert_eq!(compile_json["status"], "ok");

    let status_output = ail()
        .args(["status", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let status_json = parse_json_output(&status_output);
    assert_eq!(status_json["status"], "ok");
    assert_eq!(status_json["data"]["snapshot_count"], 2);
    assert_eq!(status_json["data"]["graph_nodes"], 1);
    assert!(status_json["data"]["head_snapshot"].is_string());
    assert!(status_json["data"]["last_change_at"].is_string());

    let context_output = ail()
        .args(["context", "fn.sample", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let context_json = parse_json_output(&context_output);
    assert_eq!(context_json["status"], "ok");
    assert_eq!(context_json["data"]["context"]["target"], "fn.sample");
    assert!(context_json["data"]["context"]["nodes"].is_array());

    let run_output = ail()
        .args(["run", "--profile", "dev", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let run_json = parse_json_output(&run_output);
    assert_eq!(run_json["status"], "ok");
    assert_eq!(run_json["data"]["outcome"], "PreflightPassed");

    dir.child(".ail/HEAD").assert(predicate::path::exists());
    dir.child(".ail/refs/branches/main")
        .assert(predicate::path::exists());
    dir.child(".ail/store/objects")
        .assert(predicate::path::is_dir());
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

#[test]
fn test_command_runs_graph_test_nodes() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    let acl = dir.child("test-runner.acl");
    acl.write_str(
        r#"change test_runner
author cli-test
description add user-facing test runner target
base 0
op create_test id=test.addition body=eq(add(20, 22), 42)
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
        .args(["test"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("PASS test.addition"))
        .stdout(predicate::str::contains("test result: 1 passed; 0 failed"));

    let test_output = ail()
        .args(["test", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let test_json = parse_json_output(&test_output);
    assert_eq!(test_json["status"], "ok");
    assert_eq!(test_json["data"]["total"], 1);
    assert_eq!(test_json["data"]["passed"], 1);
    assert_eq!(test_json["data"]["failed"], 0);
    assert_eq!(test_json["data"]["tests"][0]["name"], "test.addition");
    assert_eq!(test_json["data"]["tests"][0]["status"], "passed");
}

#[test]
fn init_branch_writes_indirect_head_and_status_shows_branch() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");

    ail()
        .args(["init", "--branch", "feature.disk"])
        .current_dir(dir.path())
        .assert()
        .success();

    let head = std::fs::read_to_string(dir.path().join(".ail").join("HEAD")).expect("read HEAD");
    assert_eq!(head, "ref: refs/branches/feature.disk\n");

    let output = ail()
        .args(["status", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let v = parse_json_output(&output);
    assert_eq!(v["data"]["branch"], "feature.disk");
}

#[test]
fn change_branch_targets_named_branch_ref() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");

    ail().arg("init").current_dir(dir.path()).assert().success();
    // Use --apply to actually create the snapshot on the named branch.
    ail()
        .args([
            "change",
            "branch-specific snapshot",
            "--branch",
            "experiment",
            "--apply",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    let branch_ref = dir
        .path()
        .join(".ail")
        .join("refs")
        .join("branches")
        .join("experiment");
    let value = std::fs::read_to_string(branch_ref).expect("read experiment ref");
    assert_eq!(value.trim().len(), 64, "branch ref must store snapshot id");
}

#[test]
fn doctor_and_gc_report_file_store_object_counts() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");

    ail().arg("init").current_dir(dir.path()).assert().success();
    ail()
        .args(["change", "doctor gc snapshot"])
        .current_dir(dir.path())
        .assert()
        .success();

    let doctor_output = ail()
        .args(["doctor", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let doctor_json = parse_json_output(&doctor_output);
    assert!(
        doctor_json["data"]["objects"]["total"]
            .as_u64()
            .unwrap_or(0)
            > 0
    );

    let gc_output = ail()
        .args(["gc", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let gc_json = parse_json_output(&gc_output);
    assert!(gc_json["data"]["objects_before"].is_number());
    assert!(gc_json["data"]["objects_after"].is_number());
    assert!(gc_json["data"]["bytes_freed"].is_number());
}

/// Spec scenario: status exits 0.
///   GIVEN any store state
///   WHEN `ail status` runs
///   THEN exit 0
#[test]
fn status_exits_zero() {
    ail().arg("status").assert().success();
}

/// Spec scenario: status JSON output has required fields.
///   GIVEN `ail status --json` runs
///   WHEN status completes
///   THEN JSON has snapshot_id and pending_changes fields
#[test]
fn status_json_output_has_required_fields() {
    let output = ail()
        .args(["status", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"].get("pending_changes").is_some(),
        "data must have pending_changes; got: {v}"
    );
    assert!(
        v["data"].get("snapshot_id").is_some(),
        "data must have snapshot_id; got: {v}"
    );
}

/// Spec scenario: inspect unknown snapshot id exits 1.
///   GIVEN <id> not in store (valid format but unknown)
///   WHEN `ail inspect snapshot <id>` runs
///   THEN exit 1; stderr contains "not found"
#[test]
fn inspect_unknown_exits_one() {
    let unknown_id = "dead".repeat(16); // 64 valid hex chars
    ail()
        .args(["inspect", "snapshot", &unknown_id])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not found"));
}

/// Spec scenario: inspect unknown kind exits 1.
///   GIVEN <kind> is unknown and <id> is not a valid 64-char hex string
///   WHEN `ail inspect <kind> <id>` runs
///   THEN exit 1
#[test]
fn inspect_invalid_format_exits_one() {
    ail()
        .args(["inspect", "unknownkind", "not-a-hex-id"])
        .assert()
        .failure()
        .code(1);
}

/// Feature-F: inspect report --json returns real Checker-derived status.
///   GIVEN no stored reports (memory store fallback to default graph)
///   WHEN `ail inspect report ver_123 --json` runs
///   THEN exit 0; data.status is a real VerificationState (NOT "accepted")
///   NOTE: old stub returned "accepted" unconditionally — this FAILS with the stub.
#[test]
fn inspect_report_json_has_real_status_not_accepted() {
    let output = ail()
        .args(["--json", "inspect", "report", "ver_123"])
        .output()
        .expect("ail must run");
    assert!(
        output.status.success(),
        "inspect report must succeed; stderr: {}",
        std::str::from_utf8(&output.stderr).unwrap_or("(non-utf8)")
    );
    let v = parse_json_output(&output);
    let status = v["data"]["status"].as_str().unwrap_or("");
    assert_ne!(
        status, "accepted",
        "inspect report status must be a real VerificationState, not the stub 'accepted'; got: {v}"
    );
    assert!(
        !status.is_empty(),
        "inspect report status must not be empty; got: {v}"
    );
}

/// Feature-F: inspect artifact --json returns real non-null hash.
///   GIVEN a compilable default graph
///   WHEN `ail inspect artifact program.wasm --json` runs
///   THEN exit 0; data.hash is a non-null string (real WASM hash)
///   NOTE: old stub returned hash:null — this FAILS with the stub.
#[test]
fn inspect_artifact_json_has_non_null_hash() {
    let output = ail()
        .args(["--json", "inspect", "artifact", "program.wasm"])
        .output()
        .expect("ail must run");
    assert!(
        output.status.success(),
        "inspect artifact must succeed; stderr: {}",
        std::str::from_utf8(&output.stderr).unwrap_or("(non-utf8)")
    );
    let v = parse_json_output(&output);
    assert!(
        !v["data"]["hash"].is_null(),
        "inspect artifact hash must be non-null (real WASM hash); got: {v}"
    );
    let hash_str = v["data"]["hash"].as_str().unwrap_or("");
    assert_eq!(
        hash_str.len(),
        64,
        "inspect artifact hash must be a 64-char hex string; got: {hash_str}"
    );
    assert!(
        v["data"]["semantic_source_map"].is_object(),
        "inspect artifact must expose computed source-map metadata; got: {v}"
    );
    assert!(
        v["data"]["capabilities_manifest"]["entries"].is_array(),
        "inspect artifact capabilities_manifest must use entries schema; got: {v}"
    );
}

/// Feature-F: inspect capability exits 1 for unknown capability.
///   GIVEN a capability name not in any registered package
///   WHEN `ail inspect capability unknown.cap` runs
///   THEN exit 1 with NotFound
///   NOTE: old stub returned exit 0 unconditionally — this FAILS with the stub.
#[test]
fn inspect_capability_unknown_exits_one() {
    ail()
        .args(["inspect", "capability", "unknown.capability.xyz"])
        .assert()
        .failure()
        .code(1);
}

/// Spec scenario: diff with missing snapshot exits 1.
///   GIVEN snapshot <a> not in store
///   WHEN `ail diff <a>..<b>` runs (range notation)
///   THEN exit 1
#[test]
fn diff_missing_snapshots_exits_one() {
    let id_a = "aa".repeat(32); // 64 hex chars
    let id_b = "bb".repeat(32);
    let range = format!("{id_a}..{id_b}");
    ail().args(["diff", &range]).assert().failure().code(1);
}

/// Spec scenario: diff with invalid format exits 1.
///   GIVEN <target> contains an invalid hex string in range notation
///   WHEN `ail diff <range>` runs
///   THEN exit 1
#[test]
fn diff_invalid_format_exits_one() {
    ail()
        .args(["diff", "bad-id..also-bad"])
        .assert()
        .failure()
        .code(1);
}

// ── G31: rollback ─────────────────────────────────────────────────────────

/// SC-R1: rollback with valid snap-id exits 0 and prints new snapshot id.
#[test]
fn rollback_valid_id_exits_zero() {
    let snap_id = "de".repeat(32); // 64 valid hex chars
    ail()
        .args(["rollback", "--to", &snap_id])
        .assert()
        .success()
        .stdout(predicate::str::contains("new snapshot"));
}

/// SC-R2: rollback with bad-id exits 1.
#[test]
fn rollback_invalid_id_exits_one() {
    ail()
        .args(["rollback", "--to", "not-a-valid-id"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not found").or(predicate::str::contains("invalid")));
}

/// SC-R3: rollback --json produces JSON with new_snapshot_id field.
#[test]
fn rollback_json_has_new_snapshot_id() {
    let snap_id = "de".repeat(32);
    let output = ail()
        .args(["rollback", "--to", &snap_id, "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["new_snapshot_id"].is_string(),
        "data.new_snapshot_id must be a string; got: {v}"
    );
}

// ── G31: rebase ───────────────────────────────────────────────────────────

/// SC-RB1: rebase with valid change-id and onto exits 0.
#[test]
fn rebase_valid_ids_exits_zero() {
    let change_id = "ab".repeat(32);
    let onto = "cd".repeat(32);
    ail()
        .args(["rebase", &change_id, "--onto", &onto])
        .assert()
        .success()
        .stdout(predicate::str::contains("rebased").or(predicate::str::contains("conflicts")));
}

/// SC-RB2: rebase with invalid change-id exits 1.
#[test]
fn rebase_invalid_change_id_exits_one() {
    ail()
        .args(["rebase", "bad-id", "--onto", &"cd".repeat(32)])
        .assert()
        .failure()
        .code(1);
}

/// SC-RB3: rebase --json produces JSON with conflicts and repair_options.
#[test]
fn rebase_json_has_conflicts_and_repair_options() {
    let change_id = "ab".repeat(32);
    let onto = "cd".repeat(32);
    let output = ail()
        .args(["rebase", &change_id, "--onto", &onto, "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["conflicts"].is_array(),
        "data.conflicts must be an array; got: {v}"
    );
    assert!(
        v["data"]["repair_options"].is_array(),
        "data.repair_options must be an array; got: {v}"
    );
}

// ── G31: merge ────────────────────────────────────────────────────────────

/// SC-M1: merge exits 0 and prints merge result.
#[test]
fn merge_exits_zero() {
    ail()
        .args(["merge", "feature.checkout", "--into", "main"])
        .assert()
        .success()
        .stdout(predicate::str::contains("merged"));
}

/// SC-M2: merge --json produces JSON with merged_snapshot_id.
#[test]
fn merge_json_has_merged_snapshot_id() {
    let output = ail()
        .args(["merge", "feature.checkout", "--into", "main", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["merged_snapshot_id"].is_string(),
        "data.merged_snapshot_id must be a string; got: {v}"
    );
}

// ── G31: refactor ─────────────────────────────────────────────────────────

/// SC-RF1: refactor exits 0 and prints ChangeSet id.
#[test]
fn refactor_exits_zero() {
    ail()
        .args(["refactor", "extract-function", "fn.checkout"])
        .assert()
        .success()
        .stdout(predicate::str::contains("change_id").or(predicate::str::contains("refactor")));
}

/// SC-RF2: refactor --json produces JSON with change_id field.
#[test]
fn refactor_json_has_change_id() {
    let output = ail()
        .args(["refactor", "extract-function", "fn.checkout", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["change_id"].is_string(),
        "data.change_id must be a string; got: {v}"
    );
}

// ── G31: approve / reject ─────────────────────────────────────────────────

/// SC-A1: approve with valid change-id exits 0.
#[test]
fn approve_valid_id_exits_zero() {
    let change_id = "aa".repeat(32);
    ail()
        .args(["approve", &change_id, "--for", "public_api_changed"])
        .assert()
        .success()
        .stdout(predicate::str::contains("approved"));
}

/// SC-A2: approve with invalid change-id exits 1.
#[test]
fn approve_invalid_id_exits_one() {
    ail()
        .args(["approve", "bad-id", "--for", "public_api_changed"])
        .assert()
        .failure()
        .code(1);
}

/// SC-A3: reject exits 0.
#[test]
fn reject_exits_zero() {
    let change_id = "aa".repeat(32);
    ail()
        .args(["reject", &change_id, "--reason", "capability too broad"])
        .assert()
        .success()
        .stdout(predicate::str::contains("rejected"));
}

/// SC-A4: approve --json produces JSON with approved and canonical_hash fields.
#[test]
fn approve_json_has_approved_and_canonical_hash() {
    let change_id = "aa".repeat(32);
    let output = ail()
        .args([
            "approve",
            &change_id,
            "--for",
            "public_api_changed",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(
        v["data"]["approved"], true,
        "data.approved must be true; got: {v}"
    );
    assert!(
        v["data"]["canonical_hash"].is_string(),
        "data.canonical_hash must be a string; got: {v}"
    );
}

/// Reject --json produces JSON with approved=false.
#[test]
fn reject_json_has_approved_false() {
    let change_id = "aa".repeat(32);
    let output = ail()
        .args(["reject", &change_id, "--reason", "too broad", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(
        v["data"]["approved"], false,
        "data.approved must be false for reject; got: {v}"
    );
}

// ── G31: policy ───────────────────────────────────────────────────────────

/// SC-P1: policy check exits 0.
#[test]
fn policy_check_exits_zero() {
    let change_id = "ab".repeat(32);
    ail()
        .args(["policy", "check", &change_id, "--profile", "prod"])
        .assert()
        .success()
        .stdout(predicate::str::contains("policy"));
}

/// SC-P2: policy explain exits 0.
#[test]
fn policy_explain_exits_zero() {
    ail()
        .args(["policy", "explain", "no_unverified_public_api"])
        .assert()
        .success()
        .stdout(predicate::str::contains("rule").or(predicate::str::contains("description")));
}

/// SC-P3: policy set exits 0.
#[test]
fn policy_set_exits_zero() {
    ail()
        .args(["policy", "set", "max_new_capabilities=2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("policy").or(predicate::str::contains("updated")));
}

/// SC-P4: policy check --json produces JSON with policy_ok field.
#[test]
fn policy_check_json_has_policy_ok() {
    let change_id = "ab".repeat(32);
    let output = ail()
        .args(["policy", "check", &change_id, "--profile", "prod", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["policy_ok"].is_boolean(),
        "data.policy_ok must be a boolean; got: {v}"
    );
}

/// policy explain --json produces JSON with rule and description.
#[test]
fn policy_explain_json_has_rule_and_description() {
    let output = ail()
        .args(["policy", "explain", "no_unverified_public_api", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["rule"].is_string(),
        "data.rule must be a string; got: {v}"
    );
    assert!(
        v["data"]["description"].is_string(),
        "data.description must be a string; got: {v}"
    );
}

/// policy set --json produces JSON with key and value.
#[test]
fn policy_set_json_has_key_and_value() {
    let output = ail()
        .args(["policy", "set", "max_new_capabilities=2", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["key"].is_string(),
        "data.key must be a string; got: {v}"
    );
    assert!(
        v["data"]["value"].is_string(),
        "data.value must be a string; got: {v}"
    );
}

// ── G31: doctor ───────────────────────────────────────────────────────────

/// SC-D1: doctor exits 0 and prints check results.
#[test]
fn doctor_exits_zero() {
    ail()
        .args(["doctor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ok").or(predicate::str::contains("graph")));
}

/// SC-D2: doctor --json produces JSON with checks array.
#[test]
fn doctor_json_has_checks_array() {
    let output = ail()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["checks"].is_array(),
        "data.checks must be an array; got: {v}"
    );
}

/// SC-D3: each check has name, status, and message fields.
#[test]
fn doctor_json_checks_have_required_fields() {
    let output = ail()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    let checks = v["data"]["checks"]
        .as_array()
        .expect("checks must be array");
    assert!(!checks.is_empty(), "doctor must report at least one check");

    for check in checks {
        assert!(
            check["name"].is_string(),
            "each check must have a name string; got: {check}"
        );
        assert!(
            check["status"].is_string(),
            "each check must have a status string; got: {check}"
        );
        assert!(
            check["message"].is_string(),
            "each check must have a message string; got: {check}"
        );
    }
}

/// G31: unknown subcommand error lists all new commands too.
#[test]
fn unknown_subcommand_lists_all_commands_including_new() {
    let stderr = ail()
        .arg("frobnicate")
        .assert()
        .failure()
        .code(2)
        .get_output()
        .clone();

    let err_text = std::str::from_utf8(&stderr.stderr).expect("stderr must be UTF-8");
    for cmd in &[
        "rollback", "rebase", "merge", "refactor", "approve", "reject", "policy", "package", "fmt",
        "test", "new", "lsp", "doctor",
    ] {
        assert!(
            err_text.contains(cmd),
            "error message must list '{cmd}'; got:\n{err_text}"
        );
    }
}

// ── ail fmt integration tests ─────────────────────────────────────────────

/// Spec scenario: ail fmt formats ACL into canonical command text.
///   GIVEN an ACL file with out-of-order ops and non-canonical function id
///   WHEN `ail fmt --file <path> --json` runs
///   THEN JSON includes formatted ACL with phase ordering and materialized defaults
#[test]
fn fmt_file_json_outputs_canonical_acl() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir");
    let acl = dir.child("change.acl");
    acl.write_str(
        "change x\nbase 0\nauthor Ana\nop verify\nop create_function id=Fn.CartTotal return=I64\nend\n",
    )
    .expect("write acl");

    let output = ail()
        .args(["fmt", "--file"])
        .arg(acl.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    let formatted = v["data"]["formatted"]
        .as_str()
        .expect("formatted must be string");
    assert!(
        formatted.contains("op create_function id=fn.cart_total return=I64 visibility=private"),
        "fmt must normalize id and materialize default visibility; got:\n{formatted}"
    );
    assert!(
        formatted.find("op create_function").unwrap() < formatted.find("op verify").unwrap(),
        "fmt must phase-order create before verify; got:\n{formatted}"
    );
    assert_eq!(v["data"]["changed"], true);
}

/// Spec scenario: ail fmt --write rewrites the file so --check passes.
#[test]
fn fmt_write_makes_check_pass() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir");
    let acl = dir.child("change.acl");
    acl.write_str("change x\nbase 0\nauthor Ana\nop verify\nend\n")
        .expect("write acl");

    ail()
        .args(["fmt", "--file"])
        .arg(acl.path())
        .arg("--check")
        .assert()
        .failure()
        .stderr(predicate::str::contains("fmt check failed"));

    ail()
        .args(["fmt", "--file"])
        .arg(acl.path())
        .arg("--write")
        .assert()
        .success();

    ail()
        .args(["fmt", "--file"])
        .arg(acl.path())
        .arg("--check")
        .assert()
        .success();
}

#[test]
fn fmt_file_json_outputs_canonical_ail_source() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn add_pair(x:Int,y:Int)->Int=add(x,y)\n\
fn main()->Int{\n\
let base=add(20,20)\n\
return if gt(base,40){add(base,2)} else {0}\n\
}\n",
        )
        .expect("write source");

    let output = ail()
        .args(["fmt", "--file"])
        .arg(source.path())
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["language"], "ail-source");
    assert_eq!(v["data"]["item_count"], 2);
    let formatted = v["data"]["formatted"]
        .as_str()
        .expect("formatted must be string");
    assert!(formatted.contains("fn add_pair(x: Int, y: Int) -> Int = add(x, y)\n"));
    assert!(formatted.contains("fn main() -> Int {\n"));
    assert!(formatted.contains("  let base = add(20, 20)\n"));
    assert!(formatted.contains("  return if gt(base, 40) { add(base, 2) } else { 0 }\n"));
}

#[test]
fn fmt_ail_source_write_makes_check_pass() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir");
    let source = dir.child("main.ail");
    source
        .write_str("fn add_pair(x:Int,y:Int)->Int=add(x,y)\n")
        .expect("write source");

    ail()
        .args(["fmt", "--file"])
        .arg(source.path())
        .arg("--check")
        .assert()
        .failure()
        .stderr(predicate::str::contains("fmt check failed"));

    ail()
        .args(["fmt", "--file"])
        .arg(source.path())
        .arg("--write")
        .assert()
        .success()
        .stdout(predicate::str::contains("items: 1"));

    ail()
        .args(["fmt", "--file"])
        .arg(source.path())
        .arg("--check")
        .assert()
        .success();
}

// ── ail link integration tests ─────────────────────────────────────────────

/// Spec scenario: ail link --help exits 0 and mentions native/linker.
///   GIVEN `ail link --help` is invoked
///   WHEN dispatch runs
///   THEN exit 0; output describes the profile flag and native object linking
#[test]
fn link_help_exits_zero_and_describes_profile_flag() {
    ail()
        .args(["link", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("profile"));
}

/// Spec scenario: ail link with no persisted artifact returns non-zero exit.
///   GIVEN an initialized project with no prior ail compile --target native
///   WHEN `ail link` runs
///   THEN exit code is 1; stderr contains 'no native artifact'
#[test]
fn link_without_compile_exits_nonzero_with_clear_error() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    dir.child(".ail").assert(predicate::path::is_dir());

    ail()
        .args(["link", "--profile", "dev"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no native artifact"));
}

/// Spec scenario: ail link --json with no artifact emits structured JSON error on stdout.
///   GIVEN an initialized project with no prior native compilation
///   WHEN `ail link --json` runs
///   THEN exit code is 1; stdout is valid JSON with status="error"; stderr has plain message
#[test]
fn link_json_no_artifact_emits_structured_error() {
    use assert_fs::TempDir;

    let dir = TempDir::new().expect("temp dir");
    ail().arg("init").current_dir(dir.path()).assert().success();

    let output = ail()
        .args(["link", "--profile", "dev", "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no native artifact"))
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(
        v["status"], "error",
        "JSON status must be 'error' when no artifact; got: {v}"
    );
    assert_eq!(
        v["data"]["error"], "no native artifact",
        "JSON data.error must be 'no native artifact'; got: {v}"
    );
    assert!(
        v["data"]["next_action"].is_string(),
        "JSON data.next_action must be present to guide the user; got: {v}"
    );
}
