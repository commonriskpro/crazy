// Tests for iter functional exec entries via call_pure_stdlib.
//
// TDD: written BEFORE T8 implementation.
// Spec: STDLIB-EXEC-ITER-1..6, STDLIB-ITER-CONTRACT-1..4
//
// fold convention: fn receives List([acc, item]) as single arg (binary encoding).

use ail_core::semantic_graph::NodeKind;
use ail_stdlib::exec::{StdlibExecError, StdlibValue, call_pure_stdlib, find_function_entry};
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

// Helper: returns a non-Bool so predicate consumers reject dishonest callbacks.
fn int_identity(v: StdlibValue) -> Result<StdlibValue, StdlibExecError> {
    match v {
        StdlibValue::Int(n) => Ok(StdlibValue::Int(n)),
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

#[test]
fn iter_any_returns_true_for_first_match() {
    let list = StdlibValue::List(vec![
        StdlibValue::Int(1),
        StdlibValue::Int(2),
        StdlibValue::Int(3),
    ]);
    let result = call_pure_stdlib("std.iter.any", &[list, StdlibValue::Function(is_even)]);
    assert_eq!(result, Ok(StdlibValue::Bool(true)));
}

#[test]
fn iter_any_empty_list_returns_false() {
    let result = call_pure_stdlib(
        "std.iter.any",
        &[StdlibValue::List(vec![]), StdlibValue::Function(is_even)],
    );
    assert_eq!(result, Ok(StdlibValue::Bool(false)));
}

#[test]
fn iter_all_returns_true_for_empty_list() {
    let result = call_pure_stdlib(
        "std.iter.all",
        &[StdlibValue::List(vec![]), StdlibValue::Function(is_even)],
    );
    assert_eq!(result, Ok(StdlibValue::Bool(true)));
}

#[test]
fn iter_all_returns_false_for_first_non_match() {
    let list = StdlibValue::List(vec![StdlibValue::Int(2), StdlibValue::Int(3)]);
    let result = call_pure_stdlib("std.iter.all", &[list, StdlibValue::Function(is_even)]);
    assert_eq!(result, Ok(StdlibValue::Bool(false)));
}

#[test]
fn iter_find_returns_first_matching_element() {
    let list = StdlibValue::List(vec![
        StdlibValue::Int(1),
        StdlibValue::Int(2),
        StdlibValue::Int(4),
    ]);
    let result = call_pure_stdlib("std.iter.find", &[list, StdlibValue::Function(is_even)]);
    assert_eq!(
        result,
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Int(2)))))
    );
}

#[test]
fn iter_find_returns_none_on_miss() {
    let list = StdlibValue::List(vec![StdlibValue::Int(1), StdlibValue::Int(3)]);
    let result = call_pure_stdlib("std.iter.find", &[list, StdlibValue::Function(is_even)]);
    assert_eq!(result, Ok(StdlibValue::Option(None)));
}

#[test]
fn iter_position_returns_zero_based_first_match() {
    let list = StdlibValue::List(vec![
        StdlibValue::Int(1),
        StdlibValue::Int(2),
        StdlibValue::Int(4),
    ]);
    let result = call_pure_stdlib("std.iter.position", &[list, StdlibValue::Function(is_even)]);
    assert_eq!(
        result,
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Int(1)))))
    );
}

#[test]
fn iter_position_returns_none_on_miss() {
    let list = StdlibValue::List(vec![StdlibValue::Int(1), StdlibValue::Int(3)]);
    let result = call_pure_stdlib("std.iter.position", &[list, StdlibValue::Function(is_even)]);
    assert_eq!(result, Ok(StdlibValue::Option(None)));
}

#[test]
fn iter_search_helpers_reject_non_bool_predicate_result() {
    let list = StdlibValue::List(vec![StdlibValue::Int(1)]);
    let result = call_pure_stdlib("std.iter.any", &[list, StdlibValue::Function(int_identity)]);
    assert_eq!(result, Err(StdlibExecError::Type { expected: "Bool" }));
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

#[test]
fn iter_search_helpers_are_registered_in_exec_table() {
    for id in [
        "std.iter.any",
        "std.iter.all",
        "std.iter.find",
        "std.iter.position",
    ] {
        assert!(
            find_function_entry(id).is_some(),
            "{id} must be registered in executable stdlib table"
        );
    }
}

// ── STDLIB-ITER-CONTRACT-1..4: v1 contract_clauses content ──────────────
//
// Prove that all four std.iter.* function entries carry honest, non-empty
// contract_clauses with at least one requires and one ensures clause.

fn iter_entry_contracts(id: &str) -> Option<(Vec<String>, Vec<String>)> {
    let reg = v1_registry_with_functions();
    reg.entries.iter().find_map(|e| {
        if e.id.0 == id && e.kind == NodeKind::Function {
            e.contract_clauses
                .as_ref()
                .map(|c| (c.requires.clone(), c.ensures.clone()))
        } else {
            None
        }
    })
}

// STDLIB-ITER-CONTRACT-1
#[test]
fn v1_iter_map_has_function_entry_with_contracts() {
    let (req, ens) = iter_entry_contracts("std.iter.map")
        .expect("std.iter.map must be a Function entry with contract_clauses");
    assert!(!req.is_empty(), "std.iter.map requires must be non-empty");
    assert!(!ens.is_empty(), "std.iter.map ensures must be non-empty");
}

// STDLIB-ITER-CONTRACT-2
#[test]
fn v1_iter_filter_has_function_entry_with_contracts() {
    let (req, ens) = iter_entry_contracts("std.iter.filter")
        .expect("std.iter.filter must be a Function entry with contract_clauses");
    assert!(
        !req.is_empty(),
        "std.iter.filter requires must be non-empty"
    );
    assert!(!ens.is_empty(), "std.iter.filter ensures must be non-empty");
}

#[test]
fn v1_iter_search_helpers_have_function_entries_with_contracts() {
    for id in [
        "std.iter.any",
        "std.iter.all",
        "std.iter.find",
        "std.iter.position",
    ] {
        let (req, ens) = iter_entry_contracts(id)
            .unwrap_or_else(|| panic!("{id} must be a Function entry with contract_clauses"));
        assert!(!req.is_empty(), "{id} requires must be non-empty");
        assert!(!ens.is_empty(), "{id} ensures must be non-empty");
    }
}

// STDLIB-ITER-CONTRACT-3: also verifies fold's binary-pair calling convention
#[test]
fn v1_iter_fold_has_function_entry_with_contracts() {
    let (req, ens) = iter_entry_contracts("std.iter.fold")
        .expect("std.iter.fold must be a Function entry with contract_clauses");
    assert!(!req.is_empty(), "std.iter.fold requires must be non-empty");
    assert!(!ens.is_empty(), "std.iter.fold ensures must be non-empty");
    assert!(
        req.iter().any(|r| r.contains("List([acc, item])")),
        "std.iter.fold requires must document the List([acc, item]) binary-pair convention; got: {req:?}"
    );
}

// STDLIB-ITER-CONTRACT-4
#[test]
fn v1_iter_traverse_has_function_entry_with_contracts() {
    let (req, ens) = iter_entry_contracts("std.iter.traverse")
        .expect("std.iter.traverse must be a Function entry with contract_clauses");
    assert!(
        !req.is_empty(),
        "std.iter.traverse requires must be non-empty"
    );
    assert!(
        !ens.is_empty(),
        "std.iter.traverse ensures must be non-empty"
    );
}
