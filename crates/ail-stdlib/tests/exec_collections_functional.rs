// Tests for collections list functional exec entries.
//
// TDD: written BEFORE T9 implementation.
// Spec: STDLIB-EXEC-COL-1..4
//
// std.collections.list.{map, filter, fold} share the same functional
// conventions as std.iter.{map, filter, fold}.

use ail_stdlib::exec::{StdlibExecError, StdlibValue, call_pure_stdlib};

fn double(v: StdlibValue) -> Result<StdlibValue, StdlibExecError> {
    match v {
        StdlibValue::Int(n) => Ok(StdlibValue::Int(n * 2)),
        _ => Err(StdlibExecError::Type { expected: "Int" }),
    }
}

fn is_even(v: StdlibValue) -> Result<StdlibValue, StdlibExecError> {
    match v {
        StdlibValue::Int(n) => Ok(StdlibValue::Bool(n % 2 == 0)),
        _ => Err(StdlibExecError::Type { expected: "Int" }),
    }
}

fn sum_fold(v: StdlibValue) -> Result<StdlibValue, StdlibExecError> {
    let StdlibValue::List(pair) = v else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    if pair.len() != 2 {
        return Err(StdlibExecError::Arity { expected: 2, actual: pair.len() });
    }
    let (StdlibValue::Int(acc), StdlibValue::Int(item)) = (&pair[0], &pair[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(acc + item))
}

// ── STDLIB-EXEC-COL-1: list.map doubles elements ──────────────────────────

#[test]
fn list_map_doubles_elements() {
    let list = StdlibValue::List(vec![StdlibValue::Int(1)]);
    let result = call_pure_stdlib(
        "std.collections.list.map",
        &[list, StdlibValue::Function(double)],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::List(vec![StdlibValue::Int(2)]))
    );
}

// Triangulate: list.map with multiple elements
#[test]
fn list_map_multiple_elements() {
    let list = StdlibValue::List(vec![
        StdlibValue::Int(2),
        StdlibValue::Int(3),
        StdlibValue::Int(4),
    ]);
    let result = call_pure_stdlib(
        "std.collections.list.map",
        &[list, StdlibValue::Function(double)],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::List(vec![
            StdlibValue::Int(4),
            StdlibValue::Int(6),
            StdlibValue::Int(8),
        ]))
    );
}

// ── STDLIB-EXEC-COL-2: list.filter retains matching elements ─────────────

#[test]
fn list_filter_retains_even_elements() {
    let list = StdlibValue::List(vec![
        StdlibValue::Int(1),
        StdlibValue::Int(2),
        StdlibValue::Int(3),
    ]);
    let result = call_pure_stdlib(
        "std.collections.list.filter",
        &[list, StdlibValue::Function(is_even)],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::List(vec![StdlibValue::Int(2)]))
    );
}

// Triangulate: filter keeps all when all match
#[test]
fn list_filter_keeps_all_even() {
    let list = StdlibValue::List(vec![StdlibValue::Int(2), StdlibValue::Int(4)]);
    let result = call_pure_stdlib(
        "std.collections.list.filter",
        &[list, StdlibValue::Function(is_even)],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::List(vec![
            StdlibValue::Int(2),
            StdlibValue::Int(4),
        ]))
    );
}

// ── STDLIB-EXEC-COL-3: list.fold sums elements ───────────────────────────

#[test]
fn list_fold_sums_two_elements() {
    let list = StdlibValue::List(vec![StdlibValue::Int(1), StdlibValue::Int(2)]);
    let result = call_pure_stdlib(
        "std.collections.list.fold",
        &[list, StdlibValue::Int(0), StdlibValue::Function(sum_fold)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(3)));
}

// Triangulate: fold with non-zero init
#[test]
fn list_fold_with_nonzero_init() {
    let list = StdlibValue::List(vec![StdlibValue::Int(1), StdlibValue::Int(2)]);
    let result = call_pure_stdlib(
        "std.collections.list.fold",
        &[list, StdlibValue::Int(10), StdlibValue::Function(sum_fold)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(13)));
}

// ── STDLIB-EXEC-COL-4: list.concat merges two lists ──────────────────────

#[test]
fn list_concat_merges_two_lists() {
    let a = StdlibValue::List(vec![StdlibValue::Int(1), StdlibValue::Int(2)]);
    let b = StdlibValue::List(vec![StdlibValue::Int(3), StdlibValue::Int(4)]);
    let result = call_pure_stdlib("std.collections.list.concat", &[a, b]);
    assert_eq!(
        result,
        Ok(StdlibValue::List(vec![
            StdlibValue::Int(1),
            StdlibValue::Int(2),
            StdlibValue::Int(3),
            StdlibValue::Int(4),
        ]))
    );
}

// Triangulate: concat with empty list
#[test]
fn list_concat_with_empty_right() {
    let a = StdlibValue::List(vec![StdlibValue::Int(1)]);
    let b = StdlibValue::List(vec![]);
    let result = call_pure_stdlib("std.collections.list.concat", &[a, b]);
    assert_eq!(
        result,
        Ok(StdlibValue::List(vec![StdlibValue::Int(1)]))
    );
}

// Type error: non-List first arg for map
#[test]
fn list_map_non_list_returns_type_error() {
    let result = call_pure_stdlib(
        "std.collections.list.map",
        &[StdlibValue::Int(1), StdlibValue::Function(double)],
    );
    assert_eq!(result, Err(StdlibExecError::Type { expected: "List" }));
}
