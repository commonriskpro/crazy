// Tests for std.bytes exec handlers: length, at, slice, concat, empty.
//
// Spec: STDLIB-EXEC-BYTES-1..5
//
// Each spec ID covers the pure exec handler behaviour and the v1 registry
// entry (id, contract_clauses presence).  Tests are written to be readable
// as living documentation of the semantics.

use ail_stdlib::exec::{StdlibExecError, StdlibValue, call_pure_stdlib};
use ail_stdlib::v1_registry_with_functions;

// ── STDLIB-EXEC-BYTES-1: std.bytes.length ────────────────────────────────

// Spec: length(b) returns Int equal to the number of bytes.
#[test]
fn bytes_length_non_empty() {
    let result = call_pure_stdlib("std.bytes.length", &[StdlibValue::Bytes(vec![1, 2, 3])]);
    assert_eq!(result, Ok(StdlibValue::Int(3)));
}

// Triangulate: empty buffer
#[test]
fn bytes_length_empty_buffer() {
    let result = call_pure_stdlib("std.bytes.length", &[StdlibValue::Bytes(vec![])]);
    assert_eq!(result, Ok(StdlibValue::Int(0)));
}

// Triangulate: single byte
#[test]
fn bytes_length_single_byte() {
    let result = call_pure_stdlib("std.bytes.length", &[StdlibValue::Bytes(vec![0xFF])]);
    assert_eq!(result, Ok(StdlibValue::Int(1)));
}

// Wrong arg type yields Type error
#[test]
fn bytes_length_wrong_type_yields_type_error() {
    let result = call_pure_stdlib("std.bytes.length", &[StdlibValue::Int(42)]);
    assert!(
        matches!(result, Err(StdlibExecError::Type { expected: "Bytes" })),
        "expected Type error for non-Bytes arg, got: {result:?}"
    );
}

// ── STDLIB-EXEC-BYTES-2: std.bytes.at ────────────────────────────────────

// Spec: at(b, i) returns Some(byte_value) when 0 <= i < length.
#[test]
fn bytes_at_first_element() {
    let result = call_pure_stdlib(
        "std.bytes.at",
        &[StdlibValue::Bytes(vec![10, 20, 30]), StdlibValue::Int(0)],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Int(10)))))
    );
}

// Triangulate: last element
#[test]
fn bytes_at_last_element() {
    let result = call_pure_stdlib(
        "std.bytes.at",
        &[StdlibValue::Bytes(vec![10, 20, 30]), StdlibValue::Int(2)],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Int(30)))))
    );
}

// Spec: byte value range is 0..=255 — max byte (0xFF = 255)
#[test]
fn bytes_at_returns_value_in_byte_range() {
    let result = call_pure_stdlib(
        "std.bytes.at",
        &[StdlibValue::Bytes(vec![0xFF]), StdlibValue::Int(0)],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Int(255)))))
    );
}

// Spec: index >= length returns None
#[test]
fn bytes_at_out_of_bounds_returns_none() {
    let result = call_pure_stdlib(
        "std.bytes.at",
        &[StdlibValue::Bytes(vec![1, 2, 3]), StdlibValue::Int(3)],
    );
    assert_eq!(result, Ok(StdlibValue::Option(None)));
}

// Spec: negative index returns None
#[test]
fn bytes_at_negative_index_returns_none() {
    let result = call_pure_stdlib(
        "std.bytes.at",
        &[StdlibValue::Bytes(vec![1, 2, 3]), StdlibValue::Int(-1)],
    );
    assert_eq!(result, Ok(StdlibValue::Option(None)));
}

// ── STDLIB-EXEC-BYTES-3: std.bytes.slice ─────────────────────────────────

// Spec: slice(b, s, e) returns Some(Bytes) with bytes [s..e] when in bounds.
#[test]
fn bytes_slice_in_bounds_middle() {
    let result = call_pure_stdlib(
        "std.bytes.slice",
        &[
            StdlibValue::Bytes(b"abcdef".to_vec()),
            StdlibValue::Int(1),
            StdlibValue::Int(4),
        ],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Bytes(
            b"bcd".to_vec()
        )))))
    );
}

// Triangulate: full slice
#[test]
fn bytes_slice_full_range() {
    let result = call_pure_stdlib(
        "std.bytes.slice",
        &[
            StdlibValue::Bytes(b"abc".to_vec()),
            StdlibValue::Int(0),
            StdlibValue::Int(3),
        ],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Bytes(
            b"abc".to_vec()
        )))))
    );
}

// Spec: start == end returns empty Bytes (not None)
#[test]
fn bytes_slice_start_equals_end_returns_empty() {
    let result = call_pure_stdlib(
        "std.bytes.slice",
        &[
            StdlibValue::Bytes(b"abc".to_vec()),
            StdlibValue::Int(1),
            StdlibValue::Int(1),
        ],
    );
    assert_eq!(
        result,
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Bytes(
            vec![]
        )))))
    );
}

// Spec: end > length returns None
#[test]
fn bytes_slice_end_out_of_bounds_returns_none() {
    let result = call_pure_stdlib(
        "std.bytes.slice",
        &[
            StdlibValue::Bytes(b"abc".to_vec()),
            StdlibValue::Int(0),
            StdlibValue::Int(10),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Option(None)));
}

// Spec: start > end returns None
#[test]
fn bytes_slice_start_greater_than_end_returns_none() {
    let result = call_pure_stdlib(
        "std.bytes.slice",
        &[
            StdlibValue::Bytes(b"abc".to_vec()),
            StdlibValue::Int(3),
            StdlibValue::Int(1),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Option(None)));
}

// Spec: negative start returns None
#[test]
fn bytes_slice_negative_start_returns_none() {
    let result = call_pure_stdlib(
        "std.bytes.slice",
        &[
            StdlibValue::Bytes(b"abc".to_vec()),
            StdlibValue::Int(-1),
            StdlibValue::Int(2),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Option(None)));
}

// ── STDLIB-EXEC-BYTES-4: std.bytes.concat ────────────────────────────────

// Spec: concat(a, b) returns Bytes containing a followed by b.
#[test]
fn bytes_concat_two_non_empty() {
    let result = call_pure_stdlib(
        "std.bytes.concat",
        &[
            StdlibValue::Bytes(b"hello".to_vec()),
            StdlibValue::Bytes(b" world".to_vec()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Bytes(b"hello world".to_vec())));
}

// Triangulate: concat with empty right
#[test]
fn bytes_concat_with_empty_right_is_identity() {
    let result = call_pure_stdlib(
        "std.bytes.concat",
        &[
            StdlibValue::Bytes(vec![1, 2, 3]),
            StdlibValue::Bytes(vec![]),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Bytes(vec![1, 2, 3])));
}

// Triangulate: concat with empty left
#[test]
fn bytes_concat_with_empty_left_is_identity() {
    let result = call_pure_stdlib(
        "std.bytes.concat",
        &[
            StdlibValue::Bytes(vec![]),
            StdlibValue::Bytes(vec![4, 5, 6]),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Bytes(vec![4, 5, 6])));
}

// Spec: both empty returns empty
#[test]
fn bytes_concat_two_empty_buffers() {
    let result = call_pure_stdlib(
        "std.bytes.concat",
        &[StdlibValue::Bytes(vec![]), StdlibValue::Bytes(vec![])],
    );
    assert_eq!(result, Ok(StdlibValue::Bytes(vec![])));
}

// ── STDLIB-EXEC-BYTES-5: std.bytes.empty ─────────────────────────────────

// Spec: empty(b) returns true when length == 0.
#[test]
fn bytes_empty_predicate_true_for_empty_buffer() {
    let result = call_pure_stdlib("std.bytes.empty", &[StdlibValue::Bytes(vec![])]);
    assert_eq!(result, Ok(StdlibValue::Bool(true)));
}

// Spec: empty(b) returns false when length > 0.
#[test]
fn bytes_empty_predicate_false_for_non_empty_buffer() {
    let result = call_pure_stdlib("std.bytes.empty", &[StdlibValue::Bytes(vec![0])]);
    assert_eq!(result, Ok(StdlibValue::Bool(false)));
}

// Triangulate: multi-byte buffer
#[test]
fn bytes_empty_predicate_false_for_multi_byte_buffer() {
    let result = call_pure_stdlib("std.bytes.empty", &[StdlibValue::Bytes(b"hello".to_vec())]);
    assert_eq!(result, Ok(StdlibValue::Bool(false)));
}

// ── STDLIB-EXEC-BYTES-V1: v1 registry entries ────────────────────────────
//
// Prove that all five functions appear in v1_registry_with_functions() with
// the correct kind (Function) and with contract_clauses populated.

fn has_function_entry_with_contracts(id: &str) -> bool {
    let reg = v1_registry_with_functions();
    use ail_core::semantic_graph::NodeKind;
    reg.entries
        .iter()
        .any(|e| e.id.0 == id && e.kind == NodeKind::Function && e.contract_clauses.is_some())
}

#[test]
fn v1_bytes_length_has_function_entry_with_contracts() {
    assert!(
        has_function_entry_with_contracts("std.bytes.length"),
        "std.bytes.length must be a Function entry with contract_clauses"
    );
}

#[test]
fn v1_bytes_at_has_function_entry_with_contracts() {
    assert!(
        has_function_entry_with_contracts("std.bytes.at"),
        "std.bytes.at must be a Function entry with contract_clauses"
    );
}

#[test]
fn v1_bytes_slice_has_function_entry_with_contracts() {
    assert!(
        has_function_entry_with_contracts("std.bytes.slice"),
        "std.bytes.slice must be a Function entry with contract_clauses"
    );
}

#[test]
fn v1_bytes_concat_has_function_entry_with_contracts() {
    assert!(
        has_function_entry_with_contracts("std.bytes.concat"),
        "std.bytes.concat must be a Function entry with contract_clauses"
    );
}

#[test]
fn v1_bytes_empty_has_function_entry_with_contracts() {
    assert!(
        has_function_entry_with_contracts("std.bytes.empty"),
        "std.bytes.empty must be a Function entry with contract_clauses"
    );
}
