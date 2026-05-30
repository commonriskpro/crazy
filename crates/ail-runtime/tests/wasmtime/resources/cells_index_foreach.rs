use crate::helpers::*;

// ── Wave 18B: CellNew/CellGet/CellSet, IndexGet, ForEach conformance ──────
//
// Spec scenarios covered (RUNTIME-CELL-1..2, RUNTIME-INDEXGET-1..2,
// RUNTIME-FOREACH-1):
//
//  RUNTIME-CELL-1: CellNew(42) followed immediately by CellGet returns the
//    initialisation value — proves the alloc+store+load round trip works end-
//    to-end through Wasmtime.
//
//  RUNTIME-CELL-2: CellNew(1) followed by CellSet(c, 10) then CellGet(c)
//    returns 10 — proves that the write overwrites the initial value and that
//    the cell pointer is stable across Let bindings.
//
//  RUNTIME-INDEXGET-1: IndexGet on a two-element list at index 0 returns the
//    first element (5) — proves the list-header skip (offset 8) and the base-
//    case formula `ptr + 8 + 0*8 = ptr + 8`.
//
//  RUNTIME-INDEXGET-2: IndexGet at index 1 returns the second element (10) —
//    proves the stride formula `ptr + 8 + 1*8 = ptr + 16`.
//
//  RUNTIME-FOREACH-1: ForEach over [1, 2, 3] with a cell accumulator yields 6
//    — proves that the inline loop binds each element into `x`, that CellGet
//    and CellSet work inside the loop body, and that ForEach as a Let value
//    produces a unit (I32 0) so the enclosing Let can sequence it with a
//    subsequent CellGet.

// RUNTIME-CELL-1
//
// fn.main = let init = 42 in let c = CellNew(init) in CellGet(c)
//
// CellNew allocates 8 bytes, stores init (42) at offset 0, returns I32 ptr.
// CellGet loads I64 from offset 0 of the ptr → 42.
#[test]
fn cell_new_get_round_trip() {
    let expr = AnfExpr::Let {
        name: "init".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(42))),
        body: Box::new(AnfExpr::Let {
            name: "c".to_string(),
            value: Box::new(AnfExpr::CellNew {
                init: "init".to_string(),
            }),
            body: Box::new(AnfExpr::CellGet {
                cell: "c".to_string(),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(42),
        "CellNew(42) followed by CellGet must return 42"
    );
}

// RUNTIME-CELL-2
//
// fn.main =
//   let init = 1 in
//   let c = CellNew(init) in
//   let v = 10 in
//   let _s = CellSet(c, v) in
//   CellGet(c)
//
// CellSet stores 10 at offset 0, overwriting the initial 1.
// CellGet then reads 10.
#[test]
fn cell_set_overwrites_initial_value() {
    let expr = AnfExpr::Let {
        name: "init".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        body: Box::new(AnfExpr::Let {
            name: "c".to_string(),
            value: Box::new(AnfExpr::CellNew {
                init: "init".to_string(),
            }),
            body: Box::new(AnfExpr::Let {
                name: "v".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
                body: Box::new(AnfExpr::Let {
                    name: "_s".to_string(),
                    value: Box::new(AnfExpr::CellSet {
                        cell: "c".to_string(),
                        value: "v".to_string(),
                    }),
                    body: Box::new(AnfExpr::CellGet {
                        cell: "c".to_string(),
                    }),
                }),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(10),
        "CellSet(c, 10) must overwrite initial 1; CellGet must return 10"
    );
}

// RUNTIME-INDEXGET-1
//
// fn.main = let lst = ListNew([5, 10]) in let i = 0 in IndexGet(lst, i)
//
// List layout: [count=2: i64, elem0=5: i64, elem1=10: i64]
// IndexGet formula: ptr + 8 + 0*8 = ptr + 8 → 5.
#[test]
fn index_get_element_at_zero() {
    let expr = AnfExpr::Let {
        name: "lst".to_string(),
        value: Box::new(AnfExpr::ListNew(vec![
            AnfExpr::Literal(LiteralValue::Int(5)),
            AnfExpr::Literal(LiteralValue::Int(10)),
        ])),
        body: Box::new(AnfExpr::Let {
            name: "i".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
            body: Box::new(AnfExpr::IndexGet {
                collection: "lst".to_string(),
                index: "i".to_string(),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(5),
        "IndexGet at index 0 of [5, 10] must return 5"
    );
}

// RUNTIME-INDEXGET-2
//
// fn.main = let lst = ListNew([5, 10]) in let i = 1 in IndexGet(lst, i)
//
// IndexGet formula: ptr + 8 + 1*8 = ptr + 16 → 10.
#[test]
fn index_get_element_at_one() {
    let expr = AnfExpr::Let {
        name: "lst".to_string(),
        value: Box::new(AnfExpr::ListNew(vec![
            AnfExpr::Literal(LiteralValue::Int(5)),
            AnfExpr::Literal(LiteralValue::Int(10)),
        ])),
        body: Box::new(AnfExpr::Let {
            name: "i".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::IndexGet {
                collection: "lst".to_string(),
                index: "i".to_string(),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(10),
        "IndexGet at index 1 of [5, 10] must return 10"
    );
}

// RUNTIME-INDEXGET-3
//
// fn.main = let lst = ListNew([5]) in let i = 1 in IndexGet(lst, i)
//
// Index 1 is out of bounds for a one-element list. This must trap before the
// element load instead of reading unrelated linear memory.
#[test]
fn index_get_out_of_bounds_traps() {
    let expr = AnfExpr::Let {
        name: "lst".to_string(),
        value: Box::new(AnfExpr::ListNew(vec![AnfExpr::Literal(LiteralValue::Int(
            5,
        ))])),
        body: Box::new(AnfExpr::Let {
            name: "i".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::IndexGet {
                collection: "lst".to_string(),
                index: "i".to_string(),
            }),
        }),
    };
    let result = try_invoke_compiler_expr(expr, "fn.index_oob");
    assert!(
        matches!(result, Err(RuntimeError::EncodingError(_))),
        "IndexGet out of bounds must trap, got {result:?}"
    );
}

// RUNTIME-INDEXGET-4
//
// Negative indices are invalid. The WASM backend uses an unsigned comparison,
// so -1 is treated as a huge unsigned value and traps before loading.
#[test]
fn index_get_negative_index_traps() {
    let expr = AnfExpr::Let {
        name: "lst".to_string(),
        value: Box::new(AnfExpr::ListNew(vec![AnfExpr::Literal(LiteralValue::Int(
            5,
        ))])),
        body: Box::new(AnfExpr::Let {
            name: "i".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(-1))),
            body: Box::new(AnfExpr::IndexGet {
                collection: "lst".to_string(),
                index: "i".to_string(),
            }),
        }),
    };
    let result = try_invoke_compiler_expr(expr, "fn.index_negative");
    assert!(
        matches!(result, Err(RuntimeError::EncodingError(_))),
        "IndexGet with a negative index must trap, got {result:?}"
    );
}

// RUNTIME-FOREACH-1
//
// fn.main =
//   let init = 0 in
//   let c    = CellNew(init) in
//   let lst  = ListNew([1, 2, 3]) in
//   let _fe  = ForEach(x in lst,
//                let cur = CellGet(c) in
//                let s   = cur + x   in
//                CellSet(c, s))       in
//   CellGet(c)
//
// ForEach iterates [1, 2, 3] and at each step adds x to the cell value:
//   step 0: 0 + 1 = 1
//   step 1: 1 + 2 = 3
//   step 2: 3 + 3 = 6
// Final CellGet returns 6.
//
// This also proves that ForEach is usable as the value in a Let binding —
// it must produce a unit (I32 0) on the WASM stack so the enclosing
// LocalSet does not underflow.
#[test]
fn foreach_accumulates_via_cell() {
    let expr = AnfExpr::Let {
        name: "init".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(0))),
        body: Box::new(AnfExpr::Let {
            name: "c".to_string(),
            value: Box::new(AnfExpr::CellNew {
                init: "init".to_string(),
            }),
            body: Box::new(AnfExpr::Let {
                name: "lst".to_string(),
                value: Box::new(AnfExpr::ListNew(vec![
                    AnfExpr::Literal(LiteralValue::Int(1)),
                    AnfExpr::Literal(LiteralValue::Int(2)),
                    AnfExpr::Literal(LiteralValue::Int(3)),
                ])),
                body: Box::new(AnfExpr::Let {
                    name: "_fe".to_string(),
                    value: Box::new(AnfExpr::ForEach {
                        binding: "x".to_string(),
                        collection: "lst".to_string(),
                        body: Box::new(AnfExpr::Let {
                            name: "cur".to_string(),
                            value: Box::new(AnfExpr::CellGet {
                                cell: "c".to_string(),
                            }),
                            body: Box::new(AnfExpr::Let {
                                name: "s".to_string(),
                                value: Box::new(AnfExpr::Call {
                                    func: "+".to_string(),
                                    args: vec!["cur".to_string(), "x".to_string()],
                                }),
                                body: Box::new(AnfExpr::CellSet {
                                    cell: "c".to_string(),
                                    value: "s".to_string(),
                                }),
                            }),
                        }),
                    }),
                    body: Box::new(AnfExpr::CellGet {
                        cell: "c".to_string(),
                    }),
                }),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(6),
        "ForEach over [1,2,3] accumulating via cell must yield 6"
    );
}

// ── Wave 19B: data-structure execution conformance ────────────────────────
//
// Spec scenarios covered (RUNTIME-RECORD-1..2, RUNTIME-FIELDUPDATE-1..2,
// RUNTIME-TUPLE-1..2, RUNTIME-MAP-1, RUNTIME-SET-1):
//
//  RUNTIME-RECORD-1: RecordNew({x:10, y:42}) + FieldGet("y") returns I64(42)
//    — proves the second field is stored at offset 8 and retrieved correctly.
//
//  RUNTIME-RECORD-2: RecordNew({x:10, y:42}) + FieldGet("x") returns I64(10)
//    — proves the first field is stored at offset 0 and retrieved correctly.
//    Together with RECORD-1 this is the RecordNew+FieldGet round-trip proof.
//
//  RUNTIME-FIELDUPDATE-1: FieldUpdate mutates "y" to 99 in-place; subsequent
//    FieldGet("y") on the original record name returns I64(99) — proves that
//    FieldUpdate stores the new value at the correct field offset and that the
//    record pointer remains valid after the mutation.
//
//  RUNTIME-FIELDUPDATE-2: After the same FieldUpdate(y←99), FieldGet("x")
//    on the original record still returns I64(10) — proves FieldUpdate does
//    not corrupt adjacent fields.
//
//  RUNTIME-TUPLE-1: TupleNew([10, 42]) + FieldGet("0") returns I64(10)
//    — proves the first tuple element is at byte offset 0.  FieldGet uses
//    the numeric field-name fallback (`field.parse::<usize>()`) because
//    TupleNew does not register a named record layout.
//
//  RUNTIME-TUPLE-2: TupleNew([10, 42]) + FieldGet("1") returns I64(42)
//    — proves the second element is at byte offset 8, confirming the 8-byte
//    stride is correct for tuple elements.
//
//  RUNTIME-MAP-1: MapNew with one key-value pair returns I32(ptr > 0)
//    — structural proof that MapNew compiles, instantiates, and allocates
//    without trapping.  Memory layout integrity requires introspection
//    infrastructure beyond what invoke_compiler_expr exposes.
//
//  RUNTIME-SET-1: SetNew with one element returns I32(ptr > 0)
//    — same structural proof as RUNTIME-MAP-1 for the SetNew constructor.

// RUNTIME-RECORD-1
//
// fn.main =
//   let r = RecordNew { fields: [("x", Literal(10)), ("y", Literal(42))] } in
//   FieldGet { record: "r", field: "y" }
//
// RecordNew layout: x at offset 0 (I64 10), y at offset 8 (I64 42).
// FieldGet("y"): record_layouts["r"] = ["x","y"] → index 1 → offset 8.
// load_i64_at(8, ptr) → I64(42).
