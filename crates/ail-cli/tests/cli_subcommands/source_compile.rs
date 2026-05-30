// Mechanical split from cli_subcommands.rs. Keep behavior-only moves in this module.
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
fn compile_file_accepts_source_list_literals() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("lists.ail");
    source
        .write_str(
            "fn main() -> Int {\n  let values: List<Int> = [1, 2 + 3, 5]\n  return values[1]\n}\n",
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
fn compile_file_accepts_source_list_len() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("list_len.ail");
    source
        .write_str("fn main() -> Int = len([1, 2 + 3, 5])\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .success();
}
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
#[test]
fn compile_file_accepts_source_set_and_map_collections() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("set_map.ail");
    source
        .write_str(
            "fn ids() -> Set<Int> = set(1, 2 + 3)\n\
fn labels() -> Map<Text, Int> = map(\"one\", 1, \"two\", 2)\n",
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
fn compile_file_accepts_source_tuple_collections() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("tuple.ail");
    source
        .write_str("fn pair() -> Tuple<Int, Text> = tuple(42, \"answer\")\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .success();
}
#[test]
fn compile_file_accepts_source_record_field_access_and_update() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("record.ail");
    source
        .write_str(
            "fn person() -> Record<age: Int, name: Text> = { age: 42, name: \"Ada\" }\n\
fn age() -> Int = person().age\n\
fn older() -> Record<age: Int, name: Text> = { ...person(), age: 43 }\n",
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
fn compile_file_rejects_source_list_element_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_list.ail");
    source
        .write_str("fn main() -> List<Int> = [1, true]\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in list element: expected Int, got Bool",
        ));
}
#[test]
fn compile_file_rejects_source_set_element_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_set.ail");
    source
        .write_str("fn ids() -> Set<Int> = set(1, true)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in set element: expected Int, got Bool",
        ));
}
#[test]
fn compile_file_rejects_source_map_value_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_map.ail");
    source
        .write_str("fn labels() -> Map<Text, Int> = map(\"one\", 1, \"two\", true)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in map value: expected Int, got Bool",
        ));
}
#[test]
fn compile_file_rejects_source_tuple_item_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_tuple.ail");
    source
        .write_str("fn pair() -> Tuple<Int, Text> = tuple(42, true)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in fn.pair: expected Tuple<Int,Text>, got Tuple<Int,Bool>",
        ));
}
#[test]
fn compile_file_rejects_source_record_field_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_record.ail");
    source
        .write_str("fn person() -> Record<age: Int, name: Text> = { age: true, name: \"Ada\" }\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in fn.person: expected Record<age:Int,name:Text>, got Record<age:Bool,name:Text>",
        ));
}
#[test]
fn compile_file_rejects_malformed_source_record_literal() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_record_literal.ail");
    source
        .write_str("fn person() -> Record<age: Int> = { age 42 }\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "record literal field requires `name: expression`",
        ));
}
#[test]
fn compile_file_rejects_record_update_spread_after_fields() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_record_update.ail");
    source
        .write_str(
            "fn person() -> Record<age: Int> = { age: 42 }\n\
fn older() -> Record<age: Int> = { age: 43, ...person() }\n",
        )
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "record update spread must appear first",
        ));
}
#[test]
fn compile_file_rejects_source_unknown_record_field() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_field.ail");
    source
        .write_str(
            "fn main() -> Int {\n  let p: Record<age: Int> = { age: 42 }\n  return p.name\n}\n",
        )
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unknown record field `name` for Record<age:Int>",
        ));
}
#[test]
fn compile_file_rejects_source_index_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_index.ail");
    source
        .write_str(
            "fn main() -> Int {\n  let values: List<Int> = [1, 2]\n  return values[true]\n}\n",
        )
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in index argument 2: expected Int, got Bool",
        ));
}
#[test]
fn compile_file_rejects_source_len_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_len.ail");
    source
        .write_str("fn main() -> Int = len(true)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in len argument 1: expected Text or List<Unknown>, got Bool",
        ));
}
#[test]
fn compile_file_accepts_source_first_or_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("first_or.ail");
    source
        .write_str("fn first(values: List<Int>) -> Int = first_or(values, 0)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .success();
}
#[test]
fn compile_file_accepts_source_last_or_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("last_or.ail");
    source
        .write_str("fn last(values: List<Int>) -> Int = last_or(values, 0)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .success();
}
#[test]
fn compile_file_rejects_source_last_or_fallback_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_last_or.ail");
    source
        .write_str("fn last(values: List<Int>) -> Int = last_or(values, true)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in if branches: expected Int, got Bool",
        ));
}
#[test]
fn compile_file_accepts_source_get_or_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("get_or.ail");
    source
        .write_str("fn item(values: List<Int>, idx: Int) -> Int = get_or(values, idx, 0)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .success();
}
#[test]
fn compile_file_rejects_source_get_or_fallback_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_get_or.ail");
    source
        .write_str("fn item(values: List<Int>, idx: Int) -> Int = get_or(values, idx, true)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in if branches: expected Int, got Bool",
        ));
}
#[test]
fn compile_file_accepts_source_is_empty_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("is_empty.ail");
    source
        .write_str(
            "fn no_items(values: List<Int>) -> Bool = is_empty(values)\n\
             fn no_text(value: Text) -> Bool = is_empty(value)\n",
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
fn compile_file_rejects_source_is_empty_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_is_empty.ail");
    source
        .write_str("fn empty(flag: Bool) -> Bool = is_empty(flag)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in len argument 1: expected Text or List<Unknown>, got Bool",
        ));
}
#[test]
fn compile_file_rejects_source_first_or_fallback_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_first_or.ail");
    source
        .write_str("fn first(values: List<Int>) -> Int = first_or(values, true)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in if branches: expected Int, got Bool",
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

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .success();
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
fn missing(input: Option<Int>) -> Bool = is_none(input)\n",
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
            "type mismatch in match pattern `Some(_)`: expected Option<Unknown>",
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
fn failed(input: Result<Int, Text>) -> Bool = is_err(input)\n",
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
            "type mismatch in match pattern `Ok(_)`: expected Result<Unknown,Unknown>",
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
