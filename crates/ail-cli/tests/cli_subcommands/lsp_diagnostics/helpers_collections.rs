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
fn lsp_diagnose_accepts_source_list_length_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn main(values: List<Int>) -> Int = list_length(values)\n\
             fn dotted(values: List<Int>) -> Int = list.length(values)\n",
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
fn lsp_diagnose_reports_source_list_length_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main(value: Text) -> Int = list.length(value)\n")
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
            .contains("type mismatch in list.length argument 1: expected List<Unknown>, got Text")
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_TYPE_SHAPE_MISMATCH"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.type.shape"
    );
}

#[test]
fn lsp_diagnose_reports_source_len_union_shape_code() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn count(value: Int) -> Int = len(value)\n")
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
            .contains("type mismatch in len argument 1: expected Text or List<Unknown>, got Int")
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_TYPE_SHAPE_MISMATCH"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.type.shape"
    );
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
        .write_str(
            "fn same(left: Text, right: Text) -> Bool = text_eq(left, right)\n\
fn dotted_same(left: Text, right: Text) -> Bool = text.eq(left, right)\n",
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
fn lsp_diagnose_accepts_source_text_trim_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn cleaned(value: Text) -> Text = text_trim(value)\n\
fn dotted_cleaned(value: Text) -> Text = text.trim(value)\n",
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
fn lsp_diagnose_accepts_source_text_length_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn main(value: Text) -> Int = text_length(value)\n\
             fn dotted(value: Text) -> Int = text.length(value)\n\
             fn dotted_short(value: Text) -> Int = text.len(value)\n",
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
fn lsp_diagnose_reports_source_text_length_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn main(values: List<Int>) -> Int = text.length(values)\n")
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
            .contains("type mismatch in text.length argument 1: expected Text, got List<Int>")
    );
}

#[test]
fn lsp_diagnose_accepts_source_text_byte_at_or_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn byte(value: Text, index: Int, fallback: Int) -> Int = text_byte_at_or(value, index, fallback)\n\
fn dotted_byte(value: Text, index: Int, fallback: Int) -> Int = text.byte_at_or(value, index, fallback)\n",
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
fn lsp_diagnose_reports_source_int_helper_arity_code() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn bounded(value: Int) -> Int = int_clamp(value)\n")
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
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_LOWER_INT_HELPER"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.lower.int"
    );
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("int_clamp requires `int_clamp(value, low, high)`")
    );
}

#[test]
fn lsp_diagnose_accepts_source_int_bounds_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn bounded(value: Int, low: Int, high: Int) -> Int = int_min(value, high) + int_max(value, low) + int_clamp(value, low, high) + int_abs_or(value, 0) + int_neg_or(value, 0) + int_add_or(value, 1, 0) + int_sub_or(value, 1, 0) + int_mul_or(value, 1, 0) + int_saturating_add(value, 1) + int_saturating_sub(value, 1) + int_saturating_mul(value, 1) + int_saturating_neg(value) + int_wrapping_add(value, 1) + int_wrapping_sub(value, 1) + int_wrapping_mul(value, 1) + int_wrapping_neg(value) + int_bit_and(value, high) + int_bit_or(value, low) + int_bit_xor(value, high) + int_bit_not(value) + int_shift_left(value, 1) + int_shift_right(value, 1) + int_shift_right_unsigned(value, 1) + int_div_or(value, 1, 0) + int_rem_or(value, 1, 0)\n\
fn dotted(value: Int, low: Int, high: Int) -> Int = int.min(value, high) + int.max(value, low) + int.clamp(value, low, high) + int.abs_or(value, 0) + int.neg_or(value, 0) + int.add_or(value, 1, 0) + int.sub_or(value, 1, 0) + int.mul_or(value, 1, 0) + int.saturating_add(value, 1) + int.saturating_sub(value, 1) + int.saturating_mul(value, 1) + int.saturating_neg(value) + int.wrapping_add(value, 1) + int.wrapping_sub(value, 1) + int.wrapping_mul(value, 1) + int.wrapping_neg(value) + int.bit_and(value, high) + int.bit_or(value, low) + int.bit_xor(value, high) + int.bit_not(value) + int.shift_left(value, 1) + int.shift_right(value, 1) + int.shift_right_unsigned(value, 1) + int.div_or(value, 1, 0) + int.rem_or(value, 1, 0)\n",
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
            "fn parsed(value: Text, fallback: Int) -> Int = text_parse_int_or(value, fallback)\n\
fn dotted_parsed(value: Text, fallback: Int) -> Int = text.parse_int_or(value, fallback)\n",
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
fn lsp_diagnose_reports_source_text_helper_arity_code() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn has(value: Text) -> Bool = text_contains(value)\n")
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
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_LOWER_TEXT_HELPER"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.lower.text"
    );
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("text_contains requires `text_contains(haystack, needle)`")
    );
}

#[test]
fn lsp_diagnose_accepts_source_text_contains_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn has(haystack: Text, needle: Text) -> Bool = text_contains(haystack, needle)\n\
fn dotted_has(haystack: Text, needle: Text) -> Bool = text.contains(haystack, needle)\n",
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
            "fn find(haystack: Text, needle: Text) -> Int = text_index_of(haystack, needle)\n\
fn dotted_find(haystack: Text, needle: Text) -> Int = text.index_of(haystack, needle)\n",
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
            "fn piece(value: Text, start: Int, length: Int) -> Text = text_slice(value, start, length)\n\
fn dotted_piece(value: Text, start: Int, length: Int) -> Text = text.slice(value, start, length)\n",
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
            "fn changed(value: Text, needle: Text, replacement: Text) -> Text = text_replace_first(value, needle, replacement)\n\
fn dotted_changed(value: Text, needle: Text, replacement: Text) -> Text = text.replace_first(value, needle, replacement)\n",
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
             fn dotted_prefixed(haystack: Text, prefix: Text) -> Bool = text.starts_with(haystack, prefix)\n\
             fn suffixed(haystack: Text, suffix: Text) -> Bool = text_ends_with(haystack, suffix)\n\
             fn dotted_suffixed(haystack: Text, suffix: Text) -> Bool = text.ends_with(haystack, suffix)\n",
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
        .write_str(
            "fn item(values: List<Int>, idx: Int) -> Option<Int> = list_get(values, idx)\n\
fn dotted_item(values: List<Int>, idx: Int) -> Option<Int> = list.get(values, idx)\n",
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
fn lsp_diagnose_reports_source_list_get_shape_code() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn item(value: Text) -> Option<Text> = list.get(value, 0)\n")
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
            .contains("type mismatch in list.get argument 1: expected List<Unknown>, got Text")
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_TYPE_SHAPE_MISMATCH"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.type.shape"
    );
}

#[test]
fn lsp_diagnose_accepts_source_is_empty_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn no_items(values: List<Int>) -> Bool = is_empty(values)\n\
             fn no_text(value: Text) -> Bool = is_empty(value)\n\
             fn no_items_named(values: List<Int>) -> Bool = list_is_empty(values)\n\
             fn no_text_named(value: Text) -> Bool = text.is_empty(value)\n",
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
fn lsp_diagnose_reports_source_is_empty_alias_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn empty(value: Text) -> Bool = list.is_empty(value)\n")
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
            .contains(
                "type mismatch in list.is_empty argument 1: expected List<Unknown>, got Text"
            )
    );
}

#[test]
fn lsp_diagnose_reports_source_is_empty_union_shape_code() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str("fn empty(value: Int) -> Bool = is_empty(value)\n")
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
            .contains(
                "type mismatch in is_empty argument 1: expected Text or List<Unknown>, got Int"
            )
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_TYPE_SHAPE_MISMATCH"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.type.shape"
    );
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
fn lsp_diagnose_accepts_source_list_mutation_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn values() -> List<Int> = list(1, 2)
fn pushed() -> List<Int> = list_push(values(), 3)
fn dotted_pushed() -> List<Int> = list.push(values(), 4)
fn merged() -> List<Int> = list_concat(values(), list(3, 4))
fn dotted_merged() -> List<Int> = list.concat(values(), list(5, 6))
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
fn lsp_diagnose_accepts_source_queue_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn queue() -> List<Int> = list(1, 2)
fn pushed() -> List<Int> = queue_push_back(queue(), 3)
fn dotted_pushed() -> List<Int> = queue.push_back(queue(), 4)
fn popped() -> Option<Tuple<Int, List<Int>>> = queue_pop_front(queue())
fn dotted_popped() -> Option<Tuple<Int, List<Int>>> = queue.pop_front(queue())
fn peeked() -> Option<Int> = queue_peek_front(queue())
fn dotted_peeked() -> Option<Int> = queue.peek_front(queue())
fn count() -> Int = queue_length(queue())
fn dotted_count() -> Int = queue.length(queue())
fn empty() -> Bool = queue_is_empty(queue())
fn dotted_empty() -> Bool = queue.is_empty(queue())
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
fn lsp_diagnose_accepts_source_set_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn ids() -> Set<Int> = set(1, 2)
fn has_two() -> Bool = set_contains(ids(), 2)
fn dotted_has_two() -> Bool = set.contains(ids(), 2)
fn count() -> Int = set_length(ids())
fn dotted_count() -> Int = set.length(ids())
fn updated() -> Set<Int> = set_insert(ids(), 3)
fn dotted_updated() -> Set<Int> = set.insert(ids(), 4)
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
fn lsp_diagnose_accepts_source_map_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            r#"fn labels() -> Map<Text, Int> = map("one", 1)
fn maybe() -> Option<Int> = map_get(labels(), "one")
fn dotted_maybe() -> Option<Int> = map.get(labels(), "one")
fn has() -> Bool = map_contains_key(labels(), "one")
fn dotted_has() -> Bool = map.contains_key(labels(), "one")
fn count() -> Int = map_length(labels())
fn dotted_count() -> Int = map.length(labels())
fn updated() -> Map<Text, Int> = map_insert(labels(), "two", 2)
fn dotted_updated() -> Map<Text, Int> = map.insert(labels(), "three", 3)
"#,
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
fn lsp_diagnose_reports_source_map_text_key_shape_code() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            r#"fn labels() -> Map<Int, Text> = map(1, "one")
fn maybe() -> Option<Text> = map.get(labels(), "one")
"#,
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
            .contains(
                "type mismatch in map.get argument 1: expected Map<Text,Unknown>, got Map<Int,Text>"
            )
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_TYPE_SHAPE_MISMATCH"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.type.shape"
    );
}

#[test]
fn lsp_diagnose_reports_source_tuple_helper_arity_code() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn pair() -> Tuple<Int, Text> = tuple(42, \"answer\")\n\
fn first() -> Option<Int> = tuple_first(pair(), 1)\n",
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
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_LOWER_TUPLE_HELPER"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.lower.tuple"
    );
    assert!(
        v["data"]["diagnostics"][0]["message"]
            .as_str()
            .expect("diagnostic message")
            .contains("tuple_first requires `tuple_first(tuple)`")
    );
}

#[test]
fn lsp_diagnose_accepts_source_tuple_collections() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn pair() -> Tuple<Int, Text> = tuple(42, \"answer\")\n\
fn pair_len() -> Int = tuple.length(pair())\n\
fn first() -> Option<Int> = tuple.first(pair())\n\
fn second() -> Option<Text> = tuple.second(pair())\n\
fn item(index: Int) -> Option<Text> = tuple.get(pair(), index)\n",
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
fn lsp_diagnose_reports_source_unknown_record_field_code() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("main.ail");
    source
        .write_str(
            "fn person() -> Record<age: Int> = { age: 42 }\nfn name() -> Text = person().name\n",
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
            .contains("unknown record field `name` for Record<age:Int>")
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["code"],
        "AIL_SOURCE_RECORD_FIELD_UNKNOWN"
    );
    assert_eq!(
        v["data"]["diagnostics"][0]["data"]["ailDiagnostic"]["category"],
        "source.record.field"
    );
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
