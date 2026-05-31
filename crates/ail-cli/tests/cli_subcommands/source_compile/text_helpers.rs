// Mechanical phase 2 split from source_compile.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use predicates::prelude::*;

#[test]
fn compile_file_accepts_source_text_concat_operator() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_concat_operator.ail");
    source
        .write_str("fn greeting(name: Text) -> Text = \"Hello, \" ++ name\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .success();
}
#[test]
fn compile_file_rejects_source_text_concat_operator_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_text_concat_operator.ail");
    source
        .write_str("fn greeting(value: Int) -> Text = \"Hello, \" ++ value\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in concat argument 2: expected Text, got Int",
        ));
}
#[test]
fn compile_file_accepts_source_text_eq_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_eq.ail");
    source
        .write_str("fn same(left: Text, right: Text) -> Bool = text_eq(left, right)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .success();
}
#[test]
fn compile_file_rejects_source_text_eq_helper_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_text_eq.ail");
    source
        .write_str("fn same(value: Int) -> Bool = text_eq(\"Hello\", value)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in text.eq argument 2: expected Text, got Int",
        ));
}
#[test]
fn compile_file_accepts_source_text_trim_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_trim.ail");
    source
        .write_str("fn cleaned(value: Text) -> Text = text_trim(value)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .success();
}

#[test]
fn compile_file_accepts_source_text_length_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_length.ail");
    source
        .write_str(
            "fn main(value: Text) -> Int = text_length(value)\n\
             fn dotted(value: Text) -> Int = text.length(value)\n",
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
fn compile_file_rejects_source_text_length_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_text_length.ail");
    source
        .write_str("fn main(values: List<Int>) -> Int = text.length(values)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in text.length argument 1: expected Text, got List<Int>",
        ));
}

#[test]
fn compile_file_rejects_source_text_trim_helper_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_text_trim.ail");
    source
        .write_str("fn cleaned(value: Int) -> Text = text_trim(value)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in text.trim argument 1: expected Text, got Int",
        ));
}
#[test]
fn compile_file_accepts_source_text_byte_at_or_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_byte_at_or.ail");
    source
        .write_str(
            "fn byte(value: Text, index: Int, fallback: Int) -> Int = text_byte_at_or(value, index, fallback)\n",
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
fn compile_file_rejects_source_text_byte_at_or_helper_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_text_byte_at_or.ail");
    source
        .write_str("fn byte(index: Text) -> Int = text_byte_at_or(\"AIL\", index, -1)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in text.byte_at_or argument 2: expected Int, got Text",
        ));
}
#[test]
fn compile_file_accepts_source_text_parse_int_or_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_parse_int_or.ail");
    source
        .write_str(
            "fn parsed(value: Text, fallback: Int) -> Int = text_parse_int_or(value, fallback)\n",
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
fn compile_file_rejects_source_text_parse_int_or_helper_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_text_parse_int_or.ail");
    source
        .write_str("fn parsed(fallback: Text) -> Int = text_parse_int_or(\"42\", fallback)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in text.parse_int_or argument 2: expected Int, got Text",
        ));
}
#[test]
fn compile_file_accepts_source_text_contains_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_contains.ail");
    source
        .write_str(
            "fn has(haystack: Text, needle: Text) -> Bool = text_contains(haystack, needle)\n",
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
fn compile_file_rejects_source_text_contains_helper_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_text_contains.ail");
    source
        .write_str("fn has(value: Int) -> Bool = text_contains(\"Hello\", value)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in text.contains argument 2: expected Text, got Int",
        ));
}
#[test]
fn compile_file_accepts_source_text_index_of_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_index_of.ail");
    source
        .write_str(
            "fn find(haystack: Text, needle: Text) -> Int = text_index_of(haystack, needle)\n",
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
fn compile_file_rejects_source_text_index_of_helper_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_text_index_of.ail");
    source
        .write_str("fn find(value: Int) -> Int = text_index_of(\"Hello\", value)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in text.index_of argument 2: expected Text, got Int",
        ));
}
#[test]
fn compile_file_accepts_source_text_slice_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_slice.ail");
    source
        .write_str(
            "fn piece(value: Text, start: Int, length: Int) -> Text = text_slice(value, start, length)\n",
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
fn compile_file_rejects_source_text_slice_helper_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_text_slice.ail");
    source
        .write_str("fn piece(length: Text) -> Text = text_slice(\"Hello\", 0, length)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in text.slice argument 3: expected Int, got Text",
        ));
}
#[test]
fn compile_file_accepts_source_text_replace_first_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_replace_first.ail");
    source
        .write_str(
            "fn changed(value: Text, needle: Text, replacement: Text) -> Text = text_replace_first(value, needle, replacement)\n",
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
fn compile_file_rejects_source_text_replace_first_helper_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_text_replace_first.ail");
    source
        .write_str(
            "fn changed(replacement: Int) -> Text = text_replace_first(\"Hello\", \"e\", replacement)\n",
        )
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in text.replace_first argument 3: expected Text, got Int",
        ));
}
#[test]
fn compile_file_accepts_source_text_boundary_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("text_boundary.ail");
    source
        .write_str(
            "fn prefixed(haystack: Text, prefix: Text) -> Bool = text_starts_with(haystack, prefix)\n\
             fn suffixed(haystack: Text, suffix: Text) -> Bool = text_ends_with(haystack, suffix)\n",
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
fn compile_file_rejects_source_text_boundary_helper_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_text_boundary.ail");
    source
        .write_str(
            "fn prefixed(value: Int) -> Bool = text_starts_with(\"Hello\", value)\n\
             fn suffixed(value: Int) -> Bool = text_ends_with(\"Hello\", value)\n",
        )
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in text.starts_with argument 2: expected Text, got Int",
        ));
}
