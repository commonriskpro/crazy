// Mechanical phase 2 split from source_compile.rs. Keep behavior-only moves in this module.
use crate::common::ail;
use predicates::prelude::*;

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
fn compile_file_accepts_source_list_length_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("list_length.ail");
    source
        .write_str(
            "fn main(values: List<Int>) -> Int = list_length(values)\n\
             fn dotted(values: List<Int>) -> Int = list.length(values)\n",
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
fn compile_file_rejects_source_list_length_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_list_length.ail");
    source
        .write_str("fn main(value: Text) -> Int = list.length(value)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in list.length argument 1: expected List<Unknown>, got Text",
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
fn compile_file_accepts_source_list_mutation_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("list_helpers.ail");
    source
        .write_str(
            "fn values() -> List<Int> = list(1, 2)
fn pushed() -> List<Int> = list_push(values(), 3)
fn merged() -> List<Int> = list_concat(values(), list(3, 4))
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
fn compile_file_rejects_source_list_push_value_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_list_push.ail");
    source
        .write_str(
            "fn pushed(values: List<Int>) -> List<Int> = list_push(values, true)
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
            "type mismatch in list.push argument 2: expected Int, got Bool",
        ));
}

#[test]
fn compile_file_rejects_source_list_concat_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_list_concat.ail");
    source
        .write_str(
            "fn merged(values: List<Int>) -> List<Int> = list_concat(values, list(true))
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
            "type mismatch in list.concat argument 2: expected List<Int>, got List<Bool>",
        ));
}
#[test]
fn compile_file_accepts_source_queue_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("queue_helpers.ail");
    source
        .write_str(
            "fn queue() -> List<Int> = list(1, 2)
fn pushed() -> List<Int> = queue_push_back(queue(), 3)
fn popped() -> Option<Tuple<Int, List<Int>>> = queue_pop_front(queue())
fn peeked() -> Option<Int> = queue_peek_front(queue())
fn count() -> Int = queue_length(queue())
fn empty() -> Bool = queue_is_empty(queue())
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
fn compile_file_rejects_source_queue_push_value_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_queue_push.ail");
    source
        .write_str(
            "fn pushed(values: List<Int>) -> List<Int> = queue_push_back(values, true)
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
            "type mismatch in queue.push_back argument 2: expected Int, got Bool",
        ));
}

#[test]
fn compile_file_rejects_source_queue_non_list() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_queue_pop.ail");
    source
        .write_str(
            "fn popped(value: Text) -> Option<Tuple<Int, List<Int>>> = queue_pop_front(value)
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
            "type mismatch in queue.pop_front argument 1: expected List<Unknown>, got Text",
        ));
}
#[test]
fn compile_file_accepts_source_set_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("set_helpers.ail");
    source
        .write_str(
            "fn ids() -> Set<Int> = set(1, 2)
fn has_two() -> Bool = set_contains(ids(), 2)
fn count() -> Int = set_length(ids())
fn updated() -> Set<Int> = set_insert(ids(), 3)
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
fn compile_file_rejects_source_set_helper_element_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_set_contains.ail");
    source
        .write_str(
            "fn has(ids: Set<Int>) -> Bool = set_contains(ids, true)
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
            "type mismatch in set.contains argument 2: expected Int, got Bool",
        ));
}

#[test]
fn compile_file_rejects_source_set_insert_value_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_set_insert.ail");
    source
        .write_str(
            "fn updated(ids: Set<Int>) -> Set<Int> = set_insert(ids, true)
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
            "type mismatch in set.insert argument 2: expected Int, got Bool",
        ));
}
#[test]
fn compile_file_accepts_source_map_helpers() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("map_helpers.ail");
    source
        .write_str(
            r#"fn labels() -> Map<Text, Int> = map("one", 1)
fn maybe() -> Option<Int> = map_get(labels(), "one")
fn has() -> Bool = map_contains_key(labels(), "one")
fn count() -> Int = map_length(labels())
fn updated() -> Map<Text, Int> = map_insert(labels(), "two", 2)
"#,
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
fn compile_file_rejects_source_map_helper_key_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_map_get_key.ail");
    source
        .write_str("fn item(labels: Map<Text, Int>) -> Option<Int> = map_get(labels, 1)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in map.get argument 2: expected Text, got Int",
        ));
}

#[test]
fn compile_file_rejects_source_map_insert_value_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_map_insert_value.ail");
    source
        .write_str(
            r#"fn updated(labels: Map<Text, Int>) -> Map<Text, Int> = map_insert(labels, "two", true)
"#,
        )
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in map.insert argument 3: expected Int, got Bool",
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
fn compile_file_accepts_source_list_get_helper() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("list_get.ail");
    source
        .write_str("fn item(values: List<Int>, idx: Int) -> Option<Int> = list_get(values, idx)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .success();
}

#[test]
fn compile_file_rejects_source_list_get_non_list() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_list_get.ail");
    source
        .write_str("fn item(value: Text, idx: Int) -> Option<Int> = list_get(value, idx)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in list.get argument 1: expected List<Unknown>, got Text",
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
             fn no_text(value: Text) -> Bool = is_empty(value)\n\
             fn no_items_named(values: List<Int>) -> Bool = list_is_empty(values)\n\
             fn no_text_named(value: Text) -> Bool = text.is_empty(value)\n",
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
            "type mismatch in is_empty argument 1: expected Text or List<Unknown>, got Bool",
        ));
}

#[test]
fn compile_file_rejects_source_is_empty_alias_type_mismatch() {
    use assert_fs::prelude::*;

    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    let source = dir.child("bad_is_empty_alias.ail");
    source
        .write_str("fn empty(value: Text) -> Bool = list.is_empty(value)\n")
        .expect("source fixture must be written");

    ail()
        .args(["compile", "--file"])
        .arg(source.path())
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "type mismatch in list.is_empty argument 1: expected List<Unknown>, got Text",
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
