// Mechanical phase 2 split from source_compile.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use predicates::prelude::*;

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
            "line 2: source item `fn.main` uses capability `log.write` without a grant",
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
fn compile_file_reports_source_import_cycles_with_chain() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let main = dir.child("main.ail");
    main.write_str("use \"./dep.ail\"\nfn main() -> Int = dep()\n")
        .expect("main fixture must be written");
    let dep = dir.child("dep.ail");
    dep.write_str("use \"./main.ail\"\nfn dep() -> Int = 42\n")
        .expect("dep fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(main.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "cyclic AIL source import detected:",
        ))
        .stderr(predicate::str::contains("main.ail ->"))
        .stderr(predicate::str::contains("dep.ail ->"));
}

#[test]
fn compile_file_rejects_duplicate_source_imports() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let dep = dir.child("math.ail");
    dep.write_str("fn helper() -> Int = 1\n")
        .expect("dep fixture must be written");
    let source = dir.child("duplicate_import.ail");
    source
        .write_str("use \"./math.ail\"\nuse \"./math.ail\"\nfn main() -> Int = helper()\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "line 2: duplicate import declaration `./math.ail`",
        ));
}

#[test]
fn compile_file_rejects_source_export_syntax() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_export.ail");
    source
        .write_str("export fn helper() -> Int = 1\nfn main() -> Int = helper()\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "line 1: unsupported source export syntax `export fn helper() -> Int = 1`",
        ))
        .stderr(predicate::str::contains(
            "imported `.ail` files expose declarations by name automatically",
        ));
}

#[test]
fn compile_file_reports_source_import_name_collisions() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let math = dir.child("math.ail");
    math.write_str("fn helper() -> Int = 1\n")
        .expect("math fixture must be written");
    let text = dir.child("text.ail");
    text.write_str("fn helper() -> Int = 2\n")
        .expect("text fixture must be written");
    let source = dir.child("main.ail");
    source
        .write_str("use \"./math.ail\"\nuse \"./text.ail\"\nfn main() -> Int = helper()\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "duplicate imported source function `fn.helper`",
        ))
        .stderr(predicate::str::contains("math.ail line 1"))
        .stderr(predicate::str::contains("text.ail line 1"));
}

#[test]
fn compile_file_reports_local_source_import_name_collisions() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let dep = dir.child("dep.ail");
    dep.write_str("const limit: Int = 1\n")
        .expect("dep fixture must be written");
    let source = dir.child("main.ail");
    source
        .write_str("use \"./dep.ail\"\nfn limit() -> Int = 2\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "duplicate imported source declaration `fn.limit`",
        ))
        .stderr(predicate::str::contains("dep.ail line 1"))
        .stderr(predicate::str::contains("main.ail line 2"));
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
