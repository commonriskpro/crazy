// Tests for executable tuple accessors.
//
// Tuple values are runtime shape, not a List convention. These accessors make
// tuple-producing stdlib functions usable without destructuring syntax.

use ail_stdlib::exec::{StdlibExecError, StdlibValue, call_pure_stdlib, find_function_entry};

fn sample_tuple() -> StdlibValue {
    StdlibValue::Tuple(vec![
        StdlibValue::Text("front".to_string()),
        StdlibValue::List(vec![StdlibValue::Text("rest".to_string())]),
    ])
}

#[test]
fn tuple_length_counts_arity() {
    let result = call_pure_stdlib("std.core.tuple.length", &[sample_tuple()]);
    assert_eq!(result, Ok(StdlibValue::Int(2)));
}

#[test]
fn tuple_get_returns_indexed_element() {
    let result = call_pure_stdlib("std.core.tuple.get", &[sample_tuple(), StdlibValue::Int(1)]);
    assert_eq!(
        result,
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::List(
            vec![StdlibValue::Text("rest".to_string()),]
        )))))
    );
}

#[test]
fn tuple_get_returns_none_for_negative_or_out_of_bounds() {
    let negative = call_pure_stdlib(
        "std.core.tuple.get",
        &[sample_tuple(), StdlibValue::Int(-1)],
    );
    let out_of_bounds =
        call_pure_stdlib("std.core.tuple.get", &[sample_tuple(), StdlibValue::Int(9)]);
    assert_eq!(negative, Ok(StdlibValue::Option(None)));
    assert_eq!(out_of_bounds, Ok(StdlibValue::Option(None)));
}

#[test]
fn tuple_first_and_second_return_options() {
    let first = call_pure_stdlib("std.core.tuple.first", &[sample_tuple()]);
    let second = call_pure_stdlib("std.core.tuple.second", &[sample_tuple()]);
    assert_eq!(
        first,
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Text(
            "front".to_string(),
        )))))
    );
    assert_eq!(
        second,
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::List(
            vec![StdlibValue::Text("rest".to_string()),]
        )))))
    );
}

#[test]
fn tuple_first_and_second_return_none_when_missing() {
    let empty = StdlibValue::Tuple(vec![]);
    let single = StdlibValue::Tuple(vec![StdlibValue::Int(1)]);
    assert_eq!(
        call_pure_stdlib("std.core.tuple.first", &[empty]),
        Ok(StdlibValue::Option(None))
    );
    assert_eq!(
        call_pure_stdlib("std.core.tuple.second", &[single]),
        Ok(StdlibValue::Option(None))
    );
}

#[test]
fn tuple_accessors_reject_non_tuple_values() {
    let result = call_pure_stdlib("std.core.tuple.length", &[StdlibValue::List(vec![])]);
    assert_eq!(result, Err(StdlibExecError::Type { expected: "Tuple" }));
}

#[test]
fn tuple_accessors_are_registered_in_exec_table() {
    for id in [
        "std.core.tuple.length",
        "std.core.tuple.get",
        "std.core.tuple.first",
        "std.core.tuple.second",
    ] {
        assert!(
            find_function_entry(id).is_some(),
            "{id} must be registered in executable stdlib table"
        );
    }
}
