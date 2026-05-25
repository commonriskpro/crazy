// Tests for iter functional exec entries via call_pure_stdlib.
//
// TDD: written BEFORE T8 implementation.
// Spec: STDLIB-EXEC-ITER-1..6, STDLIB-ITER-CONTRACT-1..4
//
// fold convention: fn receives List([acc, item]) as single arg (binary encoding).

use ail_core::semantic_graph::NodeKind;
use ail_stdlib::exec::{StdlibExecError, StdlibValue, call_pure_stdlib};
use ail_stdlib::v1_registry_with_functions;

// Helper: function that doubles an Int
fn double(v: StdlibValue) -> Result<StdlibValue, StdlibExecError> {
    match v {
        StdlibValue::Int(n) => Ok(StdlibValue::Int(n * 2)),
        _ => Err(StdlibExecError::Type { expected: "Int" }),
    }
}

// Helper: function that retains even Ints (returns Bool)
fn is_even(v: StdlibValue) -> Result<StdlibValue, StdlibExecError> {
    match v {
        StdlibValue::Int(n) => Ok(StdlibValue::Bool(n % 2 == 0)),
        _ => Err(StdlibExecError::Type { expected: "Int" }),
    }
}

// Helper: sum fold — receives List([acc, item])
fn sum_fold(v: StdlibValue) -> Result<StdlibValue, StdlibExecError> {
    let StdlibValue::List(pair) = v else {
        return Err(StdlibExecError::Type { expected: "List" });
    };
    if pair.len() != 2 {
        return Err(StdlibExecError::Arity {
            expected: 2,
            actual: pair.len(),
        });
    }
    let (StdlibValue::Int(acc), StdlibValue::Int(item)) = (&pair[0], &pair[1]) else {
        return Err(StdlibExecError::Type { expected: "Int" });
    };
    Ok(StdlibValue::Int(acc + item))
}

// Helper: traverse fn — wraps each Int in Ok
fn wrap_ok(v: StdlibValue) -> Result<StdlibValue, StdlibExecError> {
    Ok(StdlibValue::Result(Ok(Box::new(v))))
}

// Helper: traverse fn — fails on Int(2)
fn fail_on_two(v: StdlibValue) -> Result<StdlibValue, StdlibExecError> {
    match &v {
        StdlibValue::Int(2) => Ok(StdlibValue::Result(Err(Box::new(StdlibValue::Text(
            "fail".to_string(),
        ))))),
        _ => Ok(StdlibValue::Result(Ok(Box::new(v)))),
    }
}

// ── STDLIB-EXEC-ITER-1: map doubles each element ──────────────────────────

#[test]
fn iter_map_doubles_each_element() {
    let list = StdlibValue::List(vec![
        StdlibValue::Int(1),
        StdlibValue::Int(2),
        StdlibValue::Int(3),
    ]);
    let result = call_pure_stdlib("std.iter.map", &[list, StdlibValue::Function(double)]);
    assert_eq!(
        result,
        Ok(StdlibValue::List(vec![
            StdlibValue::Int(2),
            StdlibValue::Int(4),
            StdlibValue::Int(6),
        ]))
    );
}

// Triangulate: map on empty list
#[test]
fn iter_map_empty_list_returns_empty() {
    let result = call_pure_stdlib(
        "std.iter.map",
        &[StdlibValue::List(vec![]), StdlibValue::Function(double)],
    );
    assert_eq!(result, Ok(StdlibValue::List(vec![])));
}

// ── STDLIB-EXEC-ITER-2: filter retains evens ─────────────────────────────

#[test]
fn iter_filter_retains_even_elements() {
    let list = StdlibValue::List(vec![
        StdlibValue::Int(1),
        StdlibValue::Int(2),
        StdlibValue::Int(3),
    ]);
    let result = call_pure_stdlib("std.iter.filter", &[list, StdlibValue::Function(is_even)]);
    assert_eq!(result, Ok(StdlibValue::List(vec![StdlibValue::Int(2)])));
}

// Triangulate: filter all elements out
#[test]
fn iter_filter_removes_all_odd_elements() {
    let list = StdlibValue::List(vec![StdlibValue::Int(1), StdlibValue::Int(3)]);
    let result = call_pure_stdlib("std.iter.filter", &[list, StdlibValue::Function(is_even)]);
    assert_eq!(result, Ok(StdlibValue::List(vec![])));
}

// ── STDLIB-EXEC-ITER-3: fold sums elements ───────────────────────────────

#[test]
fn iter_fold_sums_list() {
    let list = StdlibValue::List(vec![
        StdlibValue::Int(1),
        StdlibValue::Int(2),
        StdlibValue::Int(3),
    ]);
    let result = call_pure_stdlib(
        "std.iter.fold",
        &[list, StdlibValue::Int(0), StdlibValue::Function(sum_fold)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(6)));
}

// Triangulate: fold on empty list returns init
#[test]
fn iter_fold_empty_list_returns_init() {
    let result = call_pure_stdlib(
        "std.iter.fold",
        &[
            StdlibValue::List(vec![]),
            StdlibValue::Int(42),
            StdlibValue::Function(sum_fold),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Int(42)));
}

// ── STDLIB-EXEC-ITER-4: traverse wraps all in Ok ─────────────────────────

#[test]
fn iter_traverse_all_ok_returns_list_in_ok() {
    let list = StdlibValue::List(vec![StdlibValue::Int(1), StdlibValue::Int(2)]);
    let result = call_pure_stdlib("std.iter.traverse", &[list, StdlibValue::Function(wrap_ok)]);
    assert_eq!(
        result,
        Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::List(vec![
            StdlibValue::Int(1),
            StdlibValue::Int(2),
        ])))))
    );
}

// ── STDLIB-EXEC-ITER-5: traverse short-circuits on Err ───────────────────

#[test]
fn iter_traverse_short_circuits_on_err() {
    let list = StdlibValue::List(vec![StdlibValue::Int(1), StdlibValue::Int(2)]);
    let result = call_pure_stdlib(
        "std.iter.traverse",
        &[list, StdlibValue::Function(fail_on_two)],
    );
    assert!(
        matches!(result, Ok(StdlibValue::Result(Err(_)))),
        "traverse must short-circuit and return Err when fn returns Err result"
    );
}

// ── STDLIB-EXEC-ITER-6: type error for non-List first arg ────────────────

#[test]
fn iter_map_non_list_arg_returns_type_error() {
    let result = call_pure_stdlib(
        "std.iter.map",
        &[StdlibValue::Int(1), StdlibValue::Function(double)],
    );
    assert_eq!(result, Err(StdlibExecError::Type { expected: "List" }));
}

// ── STDLIB-ITER-CONTRACT-1..4: v1 contract_clauses presence ──────────────
//
// Prove that all four std.iter.* function entries carry honest contract_clauses.

fn iter_entry_has_contracts(id: &str) -> bool {
    let reg = v1_registry_with_functions();
    reg.entries
        .iter()
        .any(|e| e.id.0 == id && e.kind == NodeKind::Function && e.contract_clauses.is_some())
}

// STDLIB-ITER-CONTRACT-1
#[test]
fn v1_iter_map_has_function_entry_with_contracts() {
    assert!(
        iter_entry_has_contracts("std.iter.map"),
        "std.iter.map must be a Function entry with contract_clauses"
    );
}

// STDLIB-ITER-CONTRACT-2
#[test]
fn v1_iter_filter_has_function_entry_with_contracts() {
    assert!(
        iter_entry_has_contracts("std.iter.filter"),
        "std.iter.filter must be a Function entry with contract_clauses"
    );
}

// STDLIB-ITER-CONTRACT-3
#[test]
fn v1_iter_fold_has_function_entry_with_contracts() {
    assert!(
        iter_entry_has_contracts("std.iter.fold"),
        "std.iter.fold must be a Function entry with contract_clauses"
    );
}

// STDLIB-ITER-CONTRACT-4
#[test]
fn v1_iter_traverse_has_function_entry_with_contracts() {
    assert!(
        iter_entry_has_contracts("std.iter.traverse"),
        "std.iter.traverse must be a Function entry with contract_clauses"
    );
}
