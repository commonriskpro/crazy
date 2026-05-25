// Tests for basic collection exec entries: list, map, set fundamentals.
//
// Spec: STDLIB-EXEC-COL-BASIC-1..7
//
// Covers:
//   STDLIB-EXEC-COL-BASIC-1  list.push   empty / non-empty
//   STDLIB-EXEC-COL-BASIC-2  list.get    in-bounds / out-of-bounds / negative
//   STDLIB-EXEC-COL-BASIC-3  list.length zero / after mutation via push
//   STDLIB-EXEC-COL-BASIC-4  map.insert  new key / overwrite existing key
//   STDLIB-EXEC-COL-BASIC-5  map.get     present / missing key
//   STDLIB-EXEC-COL-BASIC-6  set.insert  idempotency (new / duplicate)
//   STDLIB-EXEC-COL-BASIC-7  set.contains present / absent element

use std::collections::BTreeMap;

use ail_stdlib::exec::{StdlibExecError, StdlibValue, call_pure_stdlib};

// ── STDLIB-EXEC-COL-BASIC-1: list.push ───────────────────────────────────

#[test]
fn list_push_into_empty_returns_singleton() {
    let result = call_pure_stdlib(
        "std.collections.list.push",
        &[StdlibValue::List(vec![]), StdlibValue::Int(42)],
    );
    assert_eq!(result, Ok(StdlibValue::List(vec![StdlibValue::Int(42)])));
}

// Triangulate: push onto non-empty list appends at the end
#[test]
fn list_push_onto_non_empty_appends_at_end() {
    let list = StdlibValue::List(vec![StdlibValue::Int(1), StdlibValue::Int(2)]);
    let result = call_pure_stdlib(
        "std.collections.list.push",
        &[list, StdlibValue::Int(3)],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::List(vec![
            StdlibValue::Int(1),
            StdlibValue::Int(2),
            StdlibValue::Int(3),
        ]))
    );
}

// ── STDLIB-EXEC-COL-BASIC-2: list.get ────────────────────────────────────

#[test]
fn list_get_index_zero_returns_first_element() {
    let list = StdlibValue::List(vec![StdlibValue::Int(10), StdlibValue::Int(20)]);
    let result = call_pure_stdlib("std.collections.list.get", &[list, StdlibValue::Int(0)]);
    assert_eq!(
        result,
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Int(10)))))
    );
}

// Triangulate: in-bounds access at last valid index
#[test]
fn list_get_last_valid_index_returns_element() {
    let list = StdlibValue::List(vec![StdlibValue::Int(10), StdlibValue::Int(20)]);
    let result = call_pure_stdlib("std.collections.list.get", &[list, StdlibValue::Int(1)]);
    assert_eq!(
        result,
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Int(20)))))
    );
}

// Out of bounds returns None
#[test]
fn list_get_out_of_bounds_returns_none() {
    let list = StdlibValue::List(vec![StdlibValue::Int(5)]);
    let result = call_pure_stdlib("std.collections.list.get", &[list, StdlibValue::Int(1)]);
    assert_eq!(result, Ok(StdlibValue::Option(None)));
}

// Negative index returns None (not a panic)
#[test]
fn list_get_negative_index_returns_none() {
    let list = StdlibValue::List(vec![StdlibValue::Int(5)]);
    let result = call_pure_stdlib("std.collections.list.get", &[list, StdlibValue::Int(-1)]);
    assert_eq!(result, Ok(StdlibValue::Option(None)));
}

// ── STDLIB-EXEC-COL-BASIC-3: list.length ─────────────────────────────────

#[test]
fn list_length_empty_list_is_zero() {
    let result = call_pure_stdlib(
        "std.collections.list.length",
        &[StdlibValue::List(vec![])],
    );
    assert_eq!(result, Ok(StdlibValue::Int(0)));
}

// Triangulate: length reflects state after a push (mutation via functional push)
#[test]
fn list_length_after_push_increases_by_one() {
    let empty = StdlibValue::List(vec![]);
    let after_push = call_pure_stdlib(
        "std.collections.list.push",
        &[empty, StdlibValue::Text("x".to_string())],
    )
    .expect("push must succeed");

    let len = call_pure_stdlib("std.collections.list.length", &[after_push]);
    assert_eq!(len, Ok(StdlibValue::Int(1)));
}

// Triangulate: length of a two-element list
#[test]
fn list_length_two_elements() {
    let list = StdlibValue::List(vec![StdlibValue::Bool(true), StdlibValue::Bool(false)]);
    let result = call_pure_stdlib("std.collections.list.length", &[list]);
    assert_eq!(result, Ok(StdlibValue::Int(2)));
}

// ── STDLIB-EXEC-COL-BASIC-4: map.insert ──────────────────────────────────

#[test]
fn map_insert_new_key_adds_entry() {
    let map = StdlibValue::Map(BTreeMap::new());
    let result = call_pure_stdlib(
        "std.collections.map.insert",
        &[
            map,
            StdlibValue::Text("name".to_string()),
            StdlibValue::Text("alice".to_string()),
        ],
    );
    let mut expected = BTreeMap::new();
    expected.insert("name".to_string(), StdlibValue::Text("alice".to_string()));
    assert_eq!(result, Ok(StdlibValue::Map(expected)));
}

// Triangulate: inserting the same key overwrites the previous value
#[test]
fn map_insert_existing_key_overwrites_value() {
    let mut initial = BTreeMap::new();
    initial.insert("score".to_string(), StdlibValue::Int(10));
    let map = StdlibValue::Map(initial);

    let result = call_pure_stdlib(
        "std.collections.map.insert",
        &[
            map,
            StdlibValue::Text("score".to_string()),
            StdlibValue::Int(99),
        ],
    );
    let mut expected = BTreeMap::new();
    expected.insert("score".to_string(), StdlibValue::Int(99));
    assert_eq!(result, Ok(StdlibValue::Map(expected)));
}

// ── STDLIB-EXEC-COL-BASIC-5: map.get ─────────────────────────────────────

#[test]
fn map_get_present_key_returns_some() {
    let mut m = BTreeMap::new();
    m.insert("key".to_string(), StdlibValue::Int(7));
    let map = StdlibValue::Map(m);

    let result = call_pure_stdlib(
        "std.collections.map.get",
        &[map, StdlibValue::Text("key".to_string())],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Int(7)))))
    );
}

// Triangulate: looking up a missing key returns None
#[test]
fn map_get_missing_key_returns_none() {
    let map = StdlibValue::Map(BTreeMap::new());
    let result = call_pure_stdlib(
        "std.collections.map.get",
        &[map, StdlibValue::Text("missing".to_string())],
    );
    assert_eq!(result, Ok(StdlibValue::Option(None)));
}

// ── STDLIB-EXEC-COL-BASIC-6: set.insert (idempotency) ────────────────────

#[test]
fn set_insert_new_element_adds_to_list() {
    let set = StdlibValue::List(vec![StdlibValue::Int(1), StdlibValue::Int(2)]);
    let result = call_pure_stdlib(
        "std.collections.set.insert",
        &[set, StdlibValue::Int(3)],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::List(vec![
            StdlibValue::Int(1),
            StdlibValue::Int(2),
            StdlibValue::Int(3),
        ]))
    );
}

// Triangulate: inserting a duplicate element leaves the set unchanged
#[test]
fn set_insert_duplicate_element_is_idempotent() {
    let set = StdlibValue::List(vec![StdlibValue::Int(1), StdlibValue::Int(2)]);
    let result = call_pure_stdlib(
        "std.collections.set.insert",
        &[set, StdlibValue::Int(1)],
    );
    // Element 1 was already present — set must not grow
    assert_eq!(
        result,
        Ok(StdlibValue::List(vec![
            StdlibValue::Int(1),
            StdlibValue::Int(2),
        ]))
    );
}

// Insert into empty set produces singleton
#[test]
fn set_insert_into_empty_returns_singleton() {
    let set = StdlibValue::List(vec![]);
    let result = call_pure_stdlib(
        "std.collections.set.insert",
        &[set, StdlibValue::Text("a".to_string())],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::List(vec![StdlibValue::Text("a".to_string())]))
    );
}

// ── STDLIB-EXEC-COL-BASIC-7: set.contains ────────────────────────────────

#[test]
fn set_contains_present_element_returns_true() {
    let set = StdlibValue::List(vec![
        StdlibValue::Int(10),
        StdlibValue::Int(20),
        StdlibValue::Int(30),
    ]);
    let result = call_pure_stdlib(
        "std.collections.set.contains",
        &[set, StdlibValue::Int(20)],
    );
    assert_eq!(result, Ok(StdlibValue::Bool(true)));
}

// Triangulate: element not in the set returns false
#[test]
fn set_contains_absent_element_returns_false() {
    let set = StdlibValue::List(vec![StdlibValue::Int(10), StdlibValue::Int(20)]);
    let result = call_pure_stdlib(
        "std.collections.set.contains",
        &[set, StdlibValue::Int(99)],
    );
    assert_eq!(result, Ok(StdlibValue::Bool(false)));
}

// Triangulate: set.contains on an empty set is always false
#[test]
fn set_contains_empty_set_returns_false() {
    let set = StdlibValue::List(vec![]);
    let result = call_pure_stdlib(
        "std.collections.set.contains",
        &[set, StdlibValue::Int(1)],
    );
    assert_eq!(result, Ok(StdlibValue::Bool(false)));
}

// ── Type-error paths ──────────────────────────────────────────────────────

// list.push rejects a non-List first arg
#[test]
fn list_push_non_list_returns_type_error() {
    let result = call_pure_stdlib(
        "std.collections.list.push",
        &[StdlibValue::Int(0), StdlibValue::Int(1)],
    );
    assert_eq!(result, Err(StdlibExecError::Type { expected: "List" }));
}

// map.get rejects a non-Map first arg
#[test]
fn map_get_non_map_returns_type_error() {
    let result = call_pure_stdlib(
        "std.collections.map.get",
        &[
            StdlibValue::Int(0),
            StdlibValue::Text("k".to_string()),
        ],
    );
    assert_eq!(result, Err(StdlibExecError::Type { expected: "Map" }));
}

// set.contains rejects a non-List first arg
#[test]
fn set_contains_non_list_returns_type_error() {
    let result = call_pure_stdlib(
        "std.collections.set.contains",
        &[StdlibValue::Int(0), StdlibValue::Int(1)],
    );
    assert_eq!(result, Err(StdlibExecError::Type { expected: "List" }));
}
