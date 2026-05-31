// Mechanical phase 2 split from lsp_diagnostics.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use crate::common::parse_json_output;

#[test]
fn lsp_diagnose_accepts_source_list_literals() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn main() -> Int {\n  let values: List<Int> = [1, 2 + 3, 5]\n  return values[1]\n}\n",
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
fn lsp_diagnose_accepts_source_list_len() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main() -> Int = len([1, 2 + 3, 5])\n")
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
fn lsp_diagnose_accepts_source_text_concat_operator() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn greeting(name: Text) -> Text = \"Hello, \" ++ name\n")
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
fn lsp_diagnose_accepts_source_text_eq_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn same(left: Text, right: Text) -> Bool = text_eq(left, right)\n")
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
fn lsp_diagnose_accepts_source_text_trim_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn cleaned(value: Text) -> Text = text_trim(value)\n")
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
fn lsp_diagnose_accepts_source_text_byte_at_or_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn byte(value: Text, index: Int, fallback: Int) -> Int = text_byte_at_or(value, index, fallback)\n",
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
fn lsp_diagnose_accepts_source_int_bounds_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn bounded(value: Int, low: Int, high: Int) -> Int = int_min(value, high) + int_max(value, low) + int_clamp(value, low, high) + int_abs_or(value, 0) + int_neg_or(value, 0) + int_add_or(value, 1, 0) + int_sub_or(value, 1, 0) + int_mul_or(value, 1, 0) + int_saturating_add(value, 1) + int_saturating_sub(value, 1) + int_saturating_mul(value, 1) + int_saturating_neg(value) + int_wrapping_add(value, 1) + int_wrapping_sub(value, 1) + int_wrapping_mul(value, 1) + int_wrapping_neg(value) + int_bit_and(value, high) + int_bit_or(value, low) + int_bit_xor(value, high) + int_bit_not(value) + int_shift_left(value, 1) + int_shift_right(value, 1) + int_shift_right_unsigned(value, 1) + int_div_or(value, 1, 0) + int_rem_or(value, 1, 0)\n",
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
fn lsp_diagnose_accepts_source_text_parse_int_or_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn parsed(value: Text, fallback: Int) -> Int = text_parse_int_or(value, fallback)\n",
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
fn lsp_diagnose_accepts_source_text_contains_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn has(haystack: Text, needle: Text) -> Bool = text_contains(haystack, needle)\n",
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
fn lsp_diagnose_accepts_source_text_index_of_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn find(haystack: Text, needle: Text) -> Int = text_index_of(haystack, needle)\n",
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
fn lsp_diagnose_accepts_source_text_slice_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn piece(value: Text, start: Int, length: Int) -> Text = text_slice(value, start, length)\n",
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
fn lsp_diagnose_accepts_source_text_replace_first_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn changed(value: Text, needle: Text, replacement: Text) -> Text = text_replace_first(value, needle, replacement)\n",
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
fn lsp_diagnose_accepts_source_text_boundary_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn prefixed(haystack: Text, prefix: Text) -> Bool = text_starts_with(haystack, prefix)\n\
             fn suffixed(haystack: Text, suffix: Text) -> Bool = text_ends_with(haystack, suffix)\n",
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
fn lsp_diagnose_accepts_source_first_or_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn first(values: List<Int>) -> Int = first_or(values, 0)\n")
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
fn lsp_diagnose_accepts_source_last_or_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn last(values: List<Int>) -> Int = last_or(values, 0)\n")
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
fn lsp_diagnose_accepts_source_get_or_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn item(values: List<Int>, idx: Int) -> Int = get_or(values, idx, 0)\n")
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
fn lsp_diagnose_accepts_source_list_get_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn item(values: List<Int>, idx: Int) -> Option<Int> = list_get(values, idx)\n")
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
fn lsp_diagnose_accepts_source_is_empty_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn no_items(values: List<Int>) -> Bool = is_empty(values)\n\
             fn no_text(value: Text) -> Bool = is_empty(value)\n",
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
fn lsp_diagnose_accepts_source_set_and_map_collections() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn ids() -> Set<Int> = set(1, 2 + 3)\n\
fn labels() -> Map<Text, Int> = map(\"one\", 1, \"two\", 2)\n",
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
fn lsp_diagnose_accepts_source_tuple_collections() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn pair() -> Tuple<Int, Text> = tuple(42, \"answer\")\n")
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
fn lsp_diagnose_accepts_source_record_field_access() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn person() -> Record<age: Int, name: Text> = { age: 42, name: \"Ada\" }\n\
fn age() -> Int = person().age\n\
fn older() -> Record<age: Int, name: Text> = { ...person(), age: 43 }\n",
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
