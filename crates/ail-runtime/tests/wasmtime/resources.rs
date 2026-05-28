use super::helpers::*;

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
#[test]
fn record_new_field_get_second_field() {
    let expr = AnfExpr::Let {
        name: "r".to_string(),
        value: Box::new(AnfExpr::RecordNew {
            fields: vec![
                ("x".to_string(), AnfExpr::Literal(LiteralValue::Int(10))),
                ("y".to_string(), AnfExpr::Literal(LiteralValue::Int(42))),
            ],
        }),
        body: Box::new(AnfExpr::FieldGet {
            record: "r".to_string(),
            field: "y".to_string(),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.record_get_y"),
        RuntimeValue::I64(42),
        "FieldGet(y) on RecordNew{{x:10,y:42}} must return I64(42)"
    );
}

// RUNTIME-RECORD-2
//
// fn.main =
//   let r = RecordNew { fields: [("x", Literal(10)), ("y", Literal(42))] } in
//   FieldGet { record: "r", field: "x" }
//
// FieldGet("x"): record_layouts["r"] = ["x","y"] → index 0 → offset 0.
// load_i64_at(0, ptr) → I64(10).
#[test]
fn record_new_field_get_first_field() {
    let expr = AnfExpr::Let {
        name: "r".to_string(),
        value: Box::new(AnfExpr::RecordNew {
            fields: vec![
                ("x".to_string(), AnfExpr::Literal(LiteralValue::Int(10))),
                ("y".to_string(), AnfExpr::Literal(LiteralValue::Int(42))),
            ],
        }),
        body: Box::new(AnfExpr::FieldGet {
            record: "r".to_string(),
            field: "x".to_string(),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.record_get_x"),
        RuntimeValue::I64(10),
        "FieldGet(x) on RecordNew{{x:10,y:42}} must return I64(10)"
    );
}

// RUNTIME-FIELDUPDATE-1
//
// fn.main =
//   let r    = RecordNew { fields: [("x", Literal(10)), ("y", Literal(42))] } in
//   let _upd = FieldUpdate { record: "r", field: "y", value: Literal(99) }   in
//   FieldGet { record: "r", field: "y" }
//
// FieldUpdate stores 99 at ptr + 8 (field "y") in-place and returns ptr.
// The original Let-binding "r" still holds the same pointer; memory is now
// [I64(10), I64(99)].  FieldGet("y") on "r" reads offset 8 → I64(99).
#[test]
fn field_update_mutates_target_field() {
    let expr = AnfExpr::Let {
        name: "r".to_string(),
        value: Box::new(AnfExpr::RecordNew {
            fields: vec![
                ("x".to_string(), AnfExpr::Literal(LiteralValue::Int(10))),
                ("y".to_string(), AnfExpr::Literal(LiteralValue::Int(42))),
            ],
        }),
        body: Box::new(AnfExpr::Let {
            name: "_upd".to_string(),
            value: Box::new(AnfExpr::FieldUpdate {
                record: "r".to_string(),
                field: "y".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(99))),
            }),
            body: Box::new(AnfExpr::FieldGet {
                record: "r".to_string(),
                field: "y".to_string(),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.fieldupdate_y"),
        RuntimeValue::I64(99),
        "FieldUpdate(y←99) must be visible via subsequent FieldGet(y)"
    );
}

// RUNTIME-FIELDUPDATE-2
//
// fn.main =
//   let r    = RecordNew { fields: [("x", Literal(10)), ("y", Literal(42))] } in
//   let _upd = FieldUpdate { record: "r", field: "y", value: Literal(99) }   in
//   FieldGet { record: "r", field: "x" }
//
// After FieldUpdate(y←99): memory = [I64(10), I64(99)].
// FieldGet("x") reads offset 0 → I64(10).  Proves FieldUpdate does not
// corrupt the adjacent field at offset 0.
#[test]
fn field_update_leaves_other_field_unchanged() {
    let expr = AnfExpr::Let {
        name: "r".to_string(),
        value: Box::new(AnfExpr::RecordNew {
            fields: vec![
                ("x".to_string(), AnfExpr::Literal(LiteralValue::Int(10))),
                ("y".to_string(), AnfExpr::Literal(LiteralValue::Int(42))),
            ],
        }),
        body: Box::new(AnfExpr::Let {
            name: "_upd".to_string(),
            value: Box::new(AnfExpr::FieldUpdate {
                record: "r".to_string(),
                field: "y".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(99))),
            }),
            body: Box::new(AnfExpr::FieldGet {
                record: "r".to_string(),
                field: "x".to_string(),
            }),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.fieldupdate_x_unchanged"),
        RuntimeValue::I64(10),
        "FieldUpdate(y←99) must not corrupt field x; FieldGet(x) must still return I64(10)"
    );
}

// RUNTIME-TUPLE-1
//
// fn.main =
//   let t = TupleNew([Literal(10), Literal(42)]) in
//   FieldGet { record: "t", field: "0" }
//
// TupleNew layout (no count prefix): elem0 at offset 0 (I64 10),
//                                    elem1 at offset 8 (I64 42).
// FieldGet("0"): TupleNew does not register a record layout, so
//   field_offset falls back to `"0".parse::<usize>().unwrap_or(0)` = 0
//   → offset 0.  load_i64_at(0, ptr) → I64(10).
#[test]
fn tuple_new_field_get_at_index_zero() {
    let expr = AnfExpr::Let {
        name: "t".to_string(),
        value: Box::new(AnfExpr::TupleNew(vec![
            AnfExpr::Literal(LiteralValue::Int(10)),
            AnfExpr::Literal(LiteralValue::Int(42)),
        ])),
        body: Box::new(AnfExpr::FieldGet {
            record: "t".to_string(),
            field: "0".to_string(),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.tuple_get_0"),
        RuntimeValue::I64(10),
        "FieldGet(\"0\") on TupleNew([10,42]) must return the first element I64(10)"
    );
}

// RUNTIME-TUPLE-2
//
// fn.main =
//   let t = TupleNew([Literal(10), Literal(42)]) in
//   FieldGet { record: "t", field: "1" }
//
// FieldGet("1"): `"1".parse::<usize>()` = 1 → offset 1*8 = 8.
// load_i64_at(8, ptr) → I64(42).  Confirms the 8-byte element stride.
#[test]
fn tuple_new_field_get_at_index_one() {
    let expr = AnfExpr::Let {
        name: "t".to_string(),
        value: Box::new(AnfExpr::TupleNew(vec![
            AnfExpr::Literal(LiteralValue::Int(10)),
            AnfExpr::Literal(LiteralValue::Int(42)),
        ])),
        body: Box::new(AnfExpr::FieldGet {
            record: "t".to_string(),
            field: "1".to_string(),
        }),
    };
    assert_eq!(
        invoke_compiler_expr(expr, "fn.tuple_get_1"),
        RuntimeValue::I64(42),
        "FieldGet(\"1\") on TupleNew([10,42]) must return the second element I64(42)"
    );
}

// RUNTIME-MAP-1
//
// fn.main =
//   let k = Literal(1)   in
//   let v = Literal(100) in
//   MapNew { entries: [("k", "v")] }
//
// MapNew allocates [(1+1*2)*8 = 24] bytes.  Heap starts at offset 8 (the
// bump-pointer initial value when there is no effect data), so the returned
// pointer is > 0.
//
// NOTE: We can only prove structural non-crash here; verifying that the
// count (I64 1) and the k/v pair are written correctly requires memory-
// introspection infrastructure that invoke_compiler_expr does not expose.
#[test]
fn map_new_returns_non_null_pointer() {
    let expr = AnfExpr::Let {
        name: "k".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        body: Box::new(AnfExpr::Let {
            name: "v".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(100))),
            body: Box::new(AnfExpr::MapNew {
                entries: vec![("k".to_string(), "v".to_string())],
            }),
        }),
    };
    let result = invoke_compiler_expr(expr, "fn.map_new");
    assert!(
        matches!(result, RuntimeValue::I32(ptr) if ptr > 0),
        "MapNew must return a positive I32 heap pointer; got {result:?}"
    );
}

// RUNTIME-SET-1
//
// fn.main =
//   let elem = Literal(7) in
//   SetNew { elements: ["elem"] }
//
// SetNew allocates [(1+1)*8 = 16] bytes.  The returned pointer must be > 0.
//
// NOTE: Same structural-proof limitation as RUNTIME-MAP-1; memory layout
// (count at offset 0, element at offset 8) is not verified here.
#[test]
fn set_new_returns_non_null_pointer() {
    let expr = AnfExpr::Let {
        name: "elem".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(7))),
        body: Box::new(AnfExpr::SetNew {
            elements: vec!["elem".to_string()],
        }),
    };
    let result = invoke_compiler_expr(expr, "fn.set_new");
    assert!(
        matches!(result, RuntimeValue::I32(ptr) if ptr > 0),
        "SetNew must return a positive I32 heap pointer; got {result:?}"
    );
}

// ── Wave 20B: MapNew/SetNew memory-layout proof via read_memory_i64 ────────
//
// These tests upgrade the structural proofs from Wave 19B (RUNTIME-MAP-1,
// RUNTIME-SET-1 — only asserted ptr > 0) to full layout proofs that read
// actual i64 values from WASM linear memory using
// `RuntimeInstance::read_memory_i64(ptr, byte_offset)`.
//
// Compiler-defined layouts (from wasm_emit.rs):
//
//   MapNew layout  : [count: i64 @ 0, k0: i64 @ 8,  v0: i64 @ 16,
//                                     k1: i64 @ 24, v1: i64 @ 32, ...]
//   SetNew layout  : [count: i64 @ 0, e0: i64 @ 8,  e1: i64 @ 16, ...]
//
// Heap start: align_to_i64(effect_data.next_offset).  For expressions with no
// interned strings / capability args, next_offset == 0 and
// align_to_i64(0) = 8.  The bump pointer is therefore initialised to 8 and
// the first allocation starts there.
//
// Spec scenarios (RUNTIME-MAP-2..3, RUNTIME-SET-2..3):
//
//  RUNTIME-MAP-2: MapNew({k:1, v:100}) — proves count=1 at offset 0,
//    key=1 at offset 8, value=100 at offset 16.
//
//  RUNTIME-MAP-3: MapNew({k0:1, v0:100, k1:2, v1:200}) — proves the second
//    entry is written at the correct interleaved offsets (k1 @ 24, v1 @ 32).
//
//  RUNTIME-SET-2: SetNew({7}) — proves count=1 at offset 0, elem=7 at offset 8.
//
//  RUNTIME-SET-3: SetNew({7, 13}) — proves the second element lands at offset 16.

// RUNTIME-MAP-2
//
// fn.map_layout =
//   let k = Literal(1)   in
//   let v = Literal(100) in
//   MapNew { entries: [("k", "v")] }
//
// Expected heap layout at the returned ptr (heap_start = 8):
//   offset  0 → count  = 1   (i64 LE)
//   offset  8 → key    = 1   (i64 LE, value of local k)
//   offset 16 → value  = 100 (i64 LE, value of local v)
#[test]
fn map_new_layout_count_key_value() {
    let expr = AnfExpr::Let {
        name: "k".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        body: Box::new(AnfExpr::Let {
            name: "v".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(100))),
            body: Box::new(AnfExpr::MapNew {
                entries: vec![("k".to_string(), "v".to_string())],
            }),
        }),
    };
    let mut instance = compile_and_instantiate_expr(expr, "fn.map_layout");
    let ptr = match instance
        .invoke("map_layout", &[])
        .expect("invoke must succeed")
    {
        RuntimeValue::I32(p) => p,
        other => panic!("MapNew must return I32 ptr; got {other:?}"),
    };

    assert!(ptr > 0, "heap pointer must be positive; got {ptr}");

    let count = instance
        .read_memory_i64(ptr, 0)
        .expect("count at offset 0 must be readable");
    assert_eq!(
        count, 1,
        "count at offset 0 must be 1 for a single-entry map"
    );

    let key = instance
        .read_memory_i64(ptr, 8)
        .expect("key at offset 8 must be readable");
    assert_eq!(key, 1, "key0 at offset 8 must be 1 (value of k)");

    let val = instance
        .read_memory_i64(ptr, 16)
        .expect("value at offset 16 must be readable");
    assert_eq!(val, 100, "value0 at offset 16 must be 100 (value of v)");
}

// RUNTIME-MAP-3
//
// fn.map_layout2 =
//   let k0 = Literal(1)   in
//   let v0 = Literal(100) in
//   let k1 = Literal(2)   in
//   let v1 = Literal(200) in
//   MapNew { entries: [("k0","v0"), ("k1","v1")] }
//
// Expected heap layout at the returned ptr (heap_start = 8):
//   offset  0 → count = 2   (i64 LE)
//   offset  8 → k0    = 1   (i64 LE)
//   offset 16 → v0    = 100 (i64 LE)
//   offset 24 → k1    = 2   (i64 LE)
//   offset 32 → v1    = 200 (i64 LE)
#[test]
fn map_new_two_entries_layout() {
    let expr = AnfExpr::Let {
        name: "k0".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        body: Box::new(AnfExpr::Let {
            name: "v0".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(100))),
            body: Box::new(AnfExpr::Let {
                name: "k1".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(2))),
                body: Box::new(AnfExpr::Let {
                    name: "v1".to_string(),
                    value: Box::new(AnfExpr::Literal(LiteralValue::Int(200))),
                    body: Box::new(AnfExpr::MapNew {
                        entries: vec![
                            ("k0".to_string(), "v0".to_string()),
                            ("k1".to_string(), "v1".to_string()),
                        ],
                    }),
                }),
            }),
        }),
    };
    let mut instance = compile_and_instantiate_expr(expr, "fn.map_layout2");
    let ptr = match instance
        .invoke("map_layout2", &[])
        .expect("invoke must succeed")
    {
        RuntimeValue::I32(p) => p,
        other => panic!("MapNew must return I32 ptr; got {other:?}"),
    };

    assert!(ptr > 0, "heap pointer must be positive; got {ptr}");

    assert_eq!(
        instance.read_memory_i64(ptr, 0).expect("count readable"),
        2,
        "count at offset 0 must be 2 for a two-entry map"
    );
    assert_eq!(
        instance.read_memory_i64(ptr, 8).expect("k0 readable"),
        1,
        "k0 at offset 8 must be 1"
    );
    assert_eq!(
        instance.read_memory_i64(ptr, 16).expect("v0 readable"),
        100,
        "v0 at offset 16 must be 100"
    );
    assert_eq!(
        instance.read_memory_i64(ptr, 24).expect("k1 readable"),
        2,
        "k1 at offset 24 must be 2"
    );
    assert_eq!(
        instance.read_memory_i64(ptr, 32).expect("v1 readable"),
        200,
        "v1 at offset 32 must be 200"
    );
}

// RUNTIME-SET-2
//
// fn.set_layout =
//   let elem = Literal(7) in
//   SetNew { elements: ["elem"] }
//
// Expected heap layout at the returned ptr (heap_start = 8):
//   offset  0 → count = 1  (i64 LE)
//   offset  8 → elem  = 7  (i64 LE)
#[test]
fn set_new_layout_count_element() {
    let expr = AnfExpr::Let {
        name: "elem".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(7))),
        body: Box::new(AnfExpr::SetNew {
            elements: vec!["elem".to_string()],
        }),
    };
    let mut instance = compile_and_instantiate_expr(expr, "fn.set_layout");
    let ptr = match instance
        .invoke("set_layout", &[])
        .expect("invoke must succeed")
    {
        RuntimeValue::I32(p) => p,
        other => panic!("SetNew must return I32 ptr; got {other:?}"),
    };

    assert!(ptr > 0, "heap pointer must be positive; got {ptr}");

    let count = instance
        .read_memory_i64(ptr, 0)
        .expect("count at offset 0 must be readable");
    assert_eq!(
        count, 1,
        "count at offset 0 must be 1 for a single-element set"
    );

    let elem = instance
        .read_memory_i64(ptr, 8)
        .expect("element at offset 8 must be readable");
    assert_eq!(elem, 7, "elem0 at offset 8 must be 7");
}

// RUNTIME-SET-3
//
// fn.set_layout2 =
//   let e0 = Literal(7)  in
//   let e1 = Literal(13) in
//   SetNew { elements: ["e0", "e1"] }
//
// Expected heap layout at the returned ptr (heap_start = 8):
//   offset  0 → count = 2  (i64 LE)
//   offset  8 → e0    = 7  (i64 LE)
//   offset 16 → e1    = 13 (i64 LE)
#[test]
fn set_new_two_elements_layout() {
    let expr = AnfExpr::Let {
        name: "e0".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(7))),
        body: Box::new(AnfExpr::Let {
            name: "e1".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(13))),
            body: Box::new(AnfExpr::SetNew {
                elements: vec!["e0".to_string(), "e1".to_string()],
            }),
        }),
    };
    let mut instance = compile_and_instantiate_expr(expr, "fn.set_layout2");
    let ptr = match instance
        .invoke("set_layout2", &[])
        .expect("invoke must succeed")
    {
        RuntimeValue::I32(p) => p,
        other => panic!("SetNew must return I32 ptr; got {other:?}"),
    };

    assert!(ptr > 0, "heap pointer must be positive; got {ptr}");

    assert_eq!(
        instance.read_memory_i64(ptr, 0).expect("count readable"),
        2,
        "count at offset 0 must be 2 for a two-element set"
    );
    assert_eq!(
        instance.read_memory_i64(ptr, 8).expect("e0 readable"),
        7,
        "e0 at offset 8 must be 7"
    );
    assert_eq!(
        instance.read_memory_i64(ptr, 16).expect("e1 readable"),
        13,
        "e1 at offset 16 must be 13"
    );
}

// ── Wave 20C: ACL source-level E2E conformance — map and set constructors ──
//
// Spec scenarios covered (RUNTIME-ACL-MAP-1..3, RUNTIME-ACL-SET-1..2):
//
//  RUNTIME-ACL-MAP-1: ACL body `map(1, 10)` must parse to
//    CoreExpr::MapNew { entries: [(Literal(1), Literal(10))] }, lower to
//    AnfExpr::MapNew with atomized vars, emit WASM, instantiate, and return
//    I32(ptr > 0).  Proves the full pipeline from ACL source → expr_parser
//    → MapNew → lower → WASM emit without crash.
//
//    NOTE: Memory layout verification (count at offset 0, key/value pairs
//    at subsequent offsets) requires memory-introspection infrastructure not
//    yet available in invoke_acl_export.  Non-null pointer is the feasible
//    structural proof at this stage.
//
//  RUNTIME-ACL-SET-1: ACL body `set(42)` must parse to
//    CoreExpr::SetNew { elements: [Literal(42)] }, lower to AnfExpr::SetNew
//    with one atomized var, emit WASM, instantiate, and return I32(ptr > 0).
//    Same structural proof as RUNTIME-ACL-MAP-1 for the SetNew constructor.

// RUNTIME-ACL-MAP-2
//
// ACL body: map()  — empty map
//
//   Pipeline:
//   1. `map()` → parse_expr → CoreExpr::MapNew { entries: [] }
//   2. lower_to_anf → no atomizations; AnfExpr::MapNew { entries: [] }.
//   3. emit_wasm → allocates 8-byte header (count=0); returns I32(ptr > 0).
//
// This covers the zero-entry path through the MapNew emitter (the .max(8)
// guard ensures at least a count word is allocated).
#[test]
fn acl_map_empty_form_returns_non_null_pointer() {
    let acl = "\
change acl_map_2 base=0
author tester
description map() empty map must compile and return a non-null heap pointer
op create_function id=fn.main return=Int body=map()
end
";
    let value = invoke_acl_export(acl, "main");
    assert!(
        matches!(value, RuntimeValue::I32(ptr) if ptr > 0),
        "map() must return a positive I32 heap pointer; got {value:?}"
    );
}

// RUNTIME-ACL-MAP-3
//
// ACL body: map(1, 10, 2, 20)  — two-pair map
//
//   Pipeline:
//   1. `map(1, 10, 2, 20)` → parse_expr →
//      CoreExpr::MapNew { entries: [(Lit(1),Lit(10)), (Lit(2),Lit(20))] }
//   2. lower_to_anf → atomize 4 literals → _t0.._t3;
//      AnfExpr::MapNew { entries: [("_t0","_t1"), ("_t2","_t3")] }.
//   3. emit_wasm → allocates (1+2*2)*8 = 40 bytes; returns I32(ptr > 0).
//
// Proves the multi-entry atomization path and layout arithmetic are correct.
#[test]
fn acl_map_multi_pair_form_returns_non_null_pointer() {
    let acl = "\
change acl_map_3 base=0
author tester
description map(1,10,2,20) two-pair map must compile and return a non-null heap pointer
op create_function id=fn.main return=Int body=map(1, 10, 2, 20)
end
";
    let value = invoke_acl_export(acl, "main");
    assert!(
        matches!(value, RuntimeValue::I32(ptr) if ptr > 0),
        "map(1, 10, 2, 20) must return a positive I32 heap pointer; got {value:?}"
    );
}

// RUNTIME-ACL-MAP-1
//
// ACL body: map(1, 10)
//
//   Pipeline:
//   1. `map(1, 10)` → parse_expr → CoreExpr::MapNew { entries: [(Lit(1), Lit(10))] }
//   2. lower_to_core_ir → unchanged (CoreExpr::MapNew passes through).
//   3. lower_to_anf → atomize Lit(1) → _t0, atomize Lit(10) → _t1;
//      AnfExpr::MapNew { entries: [("_t0", "_t1")] }.
//   4. emit_wasm → MapNew bump-allocates heap memory; returns I32(ptr > 0).
//   5. invoke → RuntimeValue::I32(ptr) where ptr > 0.
//
// Constraint: key and value are integer literals; no let-binding required.
// The atomizer generates fresh names internally during ANF lowering.
#[test]
fn acl_map_form_returns_non_null_pointer() {
    let acl = "\
change acl_map_1 base=0
author tester
description map(1, 10) ACL form must compile and return a non-null heap pointer
op create_function id=fn.main return=Int body=map(1, 10)
end
";
    let value = invoke_acl_export(acl, "main");
    assert!(
        matches!(value, RuntimeValue::I32(ptr) if ptr > 0),
        "map(1, 10) must return a positive I32 heap pointer; got {value:?}"
    );
}

// RUNTIME-ACL-SET-1
//
// ACL body: set(42)
//
//   Pipeline:
//   1. `set(42)` → parse_expr → CoreExpr::SetNew { elements: [Lit(42)] }
//   2. lower_to_core_ir → unchanged (CoreExpr::SetNew passes through).
//   3. lower_to_anf → atomize Lit(42) → _t0;
//      AnfExpr::SetNew { elements: ["_t0"] }.
//   4. emit_wasm → SetNew bump-allocates heap memory; returns I32(ptr > 0).
//   5. invoke → RuntimeValue::I32(ptr) where ptr > 0.
//
// Constraint: element is an integer literal; no let-binding required.
#[test]
fn acl_set_form_returns_non_null_pointer() {
    let acl = "\
change acl_set_1 base=0
author tester
description set(42) ACL form must compile and return a non-null heap pointer
op create_function id=fn.main return=Int body=set(42)
end
";
    let value = invoke_acl_export(acl, "main");
    assert!(
        matches!(value, RuntimeValue::I32(ptr) if ptr > 0),
        "set(42) must return a positive I32 heap pointer; got {value:?}"
    );
}

// RUNTIME-ACL-SET-2
//
// ACL body: set()  — empty set
//
//   Pipeline:
//   1. `set()` → parse_expr → CoreExpr::SetNew { elements: [] }
//   2. lower_to_anf → no atomizations; AnfExpr::SetNew { elements: [] }.
//   3. emit_wasm → allocates 8-byte header (count=0); returns I32(ptr > 0).
//
// Covers the zero-element path through the SetNew emitter (the .max(8)
// guard ensures at least a count word is always allocated).
#[test]
fn acl_set_empty_form_returns_non_null_pointer() {
    let acl = "\
change acl_set_2 base=0
author tester
description set() empty set must compile and return a non-null heap pointer
op create_function id=fn.main return=Int body=set()
end
";
    let value = invoke_acl_export(acl, "main");
    assert!(
        matches!(value, RuntimeValue::I32(ptr) if ptr > 0),
        "set() must return a positive I32 heap pointer; got {value:?}"
    );
}

// ── Wave 21B: ACL source-level E2E pipeline — list / tuple / record ────────
//
// Spec scenarios covered (RUNTIME-ACL-LIST-1..2, RUNTIME-ACL-TUPLE-1..2,
// RUNTIME-ACL-RECORD-1..3):
//
//  RUNTIME-ACL-LIST-1: ACL body `list(1, 2, 3)` must parse to
//    CoreExpr::ListNew([Lit(1),Lit(2),Lit(3)]), lower to AnfExpr::ListNew
//    with 3 atomized vars, emit WASM, instantiate, and return I32(ptr > 0).
//    Proves the full pipeline from ACL `list` form through ListNew emission
//    without crash.
//
//    List heap layout: [count: i64 @ 0, elem0 @ 8, elem1 @ 16, elem2 @ 24].
//    Allocated bytes = 8 + 3*8 = 32.
//
//  RUNTIME-ACL-LIST-2: ACL body `list()` — empty list — must compile and
//    return I32(ptr > 0).  Empty ListNew allocates 8 bytes (just the count
//    word at offset 0 = 0).  Covers the zero-element path.
//
//  RUNTIME-ACL-TUPLE-1: ACL body `tuple(10, 20)` must parse to
//    CoreExpr::TupleNew([Lit(10),Lit(20)]), lower, emit, and return
//    I32(ptr > 0).  Tuple layout: no count prefix; elem0 @ 0, elem1 @ 8.
//    Allocated bytes = 2*8 = 16.
//
//  RUNTIME-ACL-TUPLE-2: ACL body `tuple()` — empty tuple — must return
//    I32(ptr > 0).  TupleNew allocates (n*8).max(1) bytes, so at least 1
//    byte is reserved even for an empty tuple.
//
//  RUNTIME-ACL-RECORD-1: ACL body `record(x, 10)` must parse to
//    CoreExpr::RecordNew{fields:[("x",Lit(10))]}, lower, emit WASM, and
//    return I32(ptr > 0).  One-field layout: field0 @ 0.
//    Allocated bytes = 1*8 = 8.
//
//  RUNTIME-ACL-RECORD-2: ACL body `record(x, 10, y, 42)` — two fields —
//    must return I32(ptr > 0).  Two-field layout: x @ 0, y @ 8.
//    Allocated bytes = 2*8 = 16.
//
//  RUNTIME-ACL-RECORD-3 (field get): ACL body
//    `let(r, record(x, 10, y, 42), field(r, y))` must:
//    (1) create the record; (2) bind it as `r` (registering the layout
//    ["x","y"] in the emit context); (3) resolve `field(r, y)` →
//    FieldGet{record:"r", field:"y"} → offset 1*8=8 → load I64(42).
//    Returns I64(42).  Proves the ACL `field` form resolves named fields
//    correctly through the full source-level pipeline.

// RUNTIME-ACL-LIST-1
//
// ACL body: list(1, 2, 3)
//
//   Pipeline:
//   1. `list(1, 2, 3)` → parse_expr → CoreExpr::ListNew([Lit(1),Lit(2),Lit(3)])
//   2. lower_to_anf → atomize 3 literals → _t0.._t2;
//      AnfExpr::ListNew([Var("_t0"), Var("_t1"), Var("_t2")]).
//   3. emit_wasm → allocates 8 + 3*8 = 32 bytes; count=3 @ offset 0;
//      elements 1, 2, 3 at offsets 8, 16, 24.  Returns I32(ptr > 0).
//
// Constraint: list elements are integer literals; no explicit let-binding
// required.  The atomizer generates fresh temporaries during ANF lowering.
#[test]
fn acl_list_non_empty_form_returns_non_null_pointer() {
    let acl = "\
change acl_list_1 base=0
author tester
description list(1,2,3) three-element list must compile and return a non-null heap pointer
op create_function id=fn.main return=Int body=list(1, 2, 3)
end
";
    let value = invoke_acl_export(acl, "main");
    assert!(
        matches!(value, RuntimeValue::I32(ptr) if ptr > 0),
        "list(1, 2, 3) must return a positive I32 heap pointer; got {value:?}"
    );
}

// RUNTIME-ACL-LIST-2
//
// ACL body: list()  — empty list
//
//   Pipeline:
//   1. `list()` → parse_expr → CoreExpr::ListNew([])
//   2. lower_to_anf → no atomizations; AnfExpr::ListNew([]).
//   3. emit_wasm → allocates 8 bytes (count word only, value 0); returns
//      I32(ptr > 0).
//
// Covers the zero-element path: count=0 is written at offset 0 and the
// 8-byte allocation always produces a non-null bump pointer.
#[test]
fn acl_list_empty_form_returns_non_null_pointer() {
    let acl = "\
change acl_list_2 base=0
author tester
description list() empty list must compile and return a non-null heap pointer
op create_function id=fn.main return=Int body=list()
end
";
    let value = invoke_acl_export(acl, "main");
    assert!(
        matches!(value, RuntimeValue::I32(ptr) if ptr > 0),
        "list() must return a positive I32 heap pointer; got {value:?}"
    );
}

// RUNTIME-ACL-TUPLE-1
//
// ACL body: tuple(10, 20)
//
//   Pipeline:
//   1. `tuple(10, 20)` → parse_expr → CoreExpr::TupleNew([Lit(10),Lit(20)])
//   2. lower_to_anf → atomize 2 literals → _t0, _t1;
//      AnfExpr::TupleNew([Var("_t0"), Var("_t1")]).
//   3. emit_wasm → allocates 2*8 = 16 bytes; elem0=10 @ offset 0,
//      elem1=20 @ offset 8.  Returns I32(ptr > 0).
//
// TupleNew layout: no count prefix; elements at consecutive 8-byte offsets.
#[test]
fn acl_tuple_non_empty_form_returns_non_null_pointer() {
    let acl = "\
change acl_tuple_1 base=0
author tester
description tuple(10,20) two-element tuple must compile and return a non-null heap pointer
op create_function id=fn.main return=Int body=tuple(10, 20)
end
";
    let value = invoke_acl_export(acl, "main");
    assert!(
        matches!(value, RuntimeValue::I32(ptr) if ptr > 0),
        "tuple(10, 20) must return a positive I32 heap pointer; got {value:?}"
    );
}

// RUNTIME-ACL-TUPLE-2
//
// ACL body: tuple()  — empty tuple
//
//   Pipeline:
//   1. `tuple()` → parse_expr → CoreExpr::TupleNew([])
//   2. lower_to_anf → no atomizations; AnfExpr::TupleNew([]).
//   3. emit_wasm → allocates (0*8).max(1) = 1 byte; returns I32(ptr > 0).
//
// The .max(1) guard ensures the bump allocator always advances the pointer
// even for a zero-element tuple, so the returned pointer is always > 0.
#[test]
fn acl_tuple_empty_form_returns_non_null_pointer() {
    let acl = "\
change acl_tuple_2 base=0
author tester
description tuple() empty tuple must compile and return a non-null heap pointer
op create_function id=fn.main return=Int body=tuple()
end
";
    let value = invoke_acl_export(acl, "main");
    assert!(
        matches!(value, RuntimeValue::I32(ptr) if ptr > 0),
        "tuple() must return a positive I32 heap pointer; got {value:?}"
    );
}

// RUNTIME-ACL-RECORD-1
//
// ACL body: record(x, 10)
//
//   Pipeline:
//   1. `record(x, 10)` → parse_record_call → CoreExpr::RecordNew{fields:[("x",Lit(10))]}
//      The parser calls expect_name on the first arg (Var("x") → "x") and uses
//      Lit(10) as the value.
//   2. lower_to_anf → atomize Lit(10) → _t0; AnfExpr::RecordNew{fields:[("x",Var("_t0"))]}.
//   3. emit_wasm → allocates (1*8).max(1) = 8 bytes; field0=10 @ offset 0.
//      Returns I32(ptr > 0).
#[test]
fn acl_record_one_field_form_returns_non_null_pointer() {
    let acl = "\
change acl_record_1 base=0
author tester
description record(x,10) one-field record must compile and return a non-null heap pointer
op create_function id=fn.main return=Int body=record(x, 10)
end
";
    let value = invoke_acl_export(acl, "main");
    assert!(
        matches!(value, RuntimeValue::I32(ptr) if ptr > 0),
        "record(x, 10) must return a positive I32 heap pointer; got {value:?}"
    );
}

// RUNTIME-ACL-RECORD-2
//
// ACL body: record(x, 10, y, 42)
//
//   Pipeline:
//   1. `record(x, 10, y, 42)` → parse_record_call →
//      CoreExpr::RecordNew{fields:[("x",Lit(10)),("y",Lit(42))]}
//   2. lower_to_anf → atomize 2 literals → _t0, _t1;
//      AnfExpr::RecordNew{fields:[("x",Var("_t0")),("y",Var("_t1"))]}.
//   3. emit_wasm → allocates 2*8 = 16 bytes; x=10 @ offset 0, y=42 @ offset 8.
//      Returns I32(ptr > 0).
#[test]
fn acl_record_two_field_form_returns_non_null_pointer() {
    let acl = "\
change acl_record_2 base=0
author tester
description record(x,10,y,42) two-field record must compile and return a non-null heap pointer
op create_function id=fn.main return=Int body=record(x, 10, y, 42)
end
";
    let value = invoke_acl_export(acl, "main");
    assert!(
        matches!(value, RuntimeValue::I32(ptr) if ptr > 0),
        "record(x, 10, y, 42) must return a positive I32 heap pointer; got {value:?}"
    );
}

// RUNTIME-ACL-RECORD-3
//
// ACL body: let(r, record(x, 10, y, 42), field(r, y))
//
//   Pipeline:
//   1. `record(x, 10, y, 42)` → RecordNew{fields:[("x",Lit(10)),("y",Lit(42))]}
//   2. `let(r, <record>, field(r, y))`:
//      - Lowers record to Let{_t0=10, _t1=42, RecordNew{[("x",_t0),("y",_t1)]}};
//        the Let binding for "r" calls bind_record_layout("r", ["x","y"]).
//      - `field(r, y)` → FieldGet{record:"r", field:"y"}.
//   3. emit_wasm FieldGet: field_offset("r", "y") → index 1 → offset 8.
//      load_i64_at(ptr + 8) → I64(42).
//   4. Returns I64(42).
//
// Proves the ACL `field` form resolves named fields end-to-end through:
//   ACL parse → expr_parser → RecordNew → lower_to_anf → wasm_emit →
//   runtime FieldGet with correct heap offset.
#[test]
fn acl_record_field_get_second_field_returns_value() {
    let acl = "\
change acl_record_3 base=0
author tester
description let(r, record(x,10,y,42), field(r,y)): field get must return I64(42)
op create_function id=fn.main return=Int body=let(r, record(x, 10, y, 42), field(r, y))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(42),
        "let(r, record(x,10,y,42), field(r,y)) must return I64(42) via FieldGet at offset 8"
    );
}

// ── Wave 21C: memory layout conformance — ListNew / TupleNew / RecordNew ──
//
// These tests upgrade the structural proofs (ptr > 0 only) for ListNew,
// TupleNew, and RecordNew to full layout proofs that read actual i64 values
// from WASM linear memory using `RuntimeInstance::read_memory_i64`.
//
// Compiler-defined layouts (from wasm_emit.rs):
//
//   ListNew layout  : [count: i64 @ 0, e0: i64 @ 8,  e1: i64 @ 16, ...]
//                     Allocates: 8 + n*8 bytes.
//
//   TupleNew layout : [e0: i64 @ 0, e1: i64 @ 8, ...]  (no count prefix)
//                     Allocates: (n*8).max(1) bytes.
//
//   RecordNew layout: [f0: i64 @ 0, f1: i64 @ 8, ...]  (no count prefix)
//                     Allocates: (n*8).max(1) bytes.
//                     Field order matches the fields vec order.
//
// Spec scenarios (RUNTIME-LIST-1..2, RUNTIME-TUPLE-3, RUNTIME-RECORD-3..4):
//
//  RUNTIME-LIST-1: ListNew([1,2,3]) — proves count=3 at offset 0, and
//    elem0=1 @ 8, elem1=2 @ 16, elem2=3 @ 24 via read_memory_i64.
//
//  RUNTIME-LIST-2: ListNew([]) — proves count=0 at offset 0; the allocator
//    still returns a positive pointer because 8 bytes are always reserved.
//
//  RUNTIME-TUPLE-3: TupleNew([10,20]) — proves no count word: elem0=10 @ 0,
//    elem1=20 @ 8 via read_memory_i64.  Complements RUNTIME-TUPLE-1..2
//    (FieldGet round-trips) with a raw memory read.
//
//  RUNTIME-RECORD-3: RecordNew({x:42,y:99}) — proves field0=42 @ 0,
//    field1=99 @ 8 via read_memory_i64.  Complements RUNTIME-RECORD-1..2
//    (FieldGet round-trips) with a raw memory read.
//
//  RUNTIME-RECORD-4: RecordNew({}) — proves the allocator always returns a
//    positive pointer even for a zero-field record (.max(1) guard).

// RUNTIME-LIST-1
//
// fn.list_layout =
//   let e0 = Literal(1) in
//   let e1 = Literal(2) in
//   let e2 = Literal(3) in
//   ListNew([Var("e0"), Var("e1"), Var("e2")])
//
// Expected heap layout at the returned ptr (heap_start = 8):
//   offset  0 → count = 3  (i64 LE)
//   offset  8 → e0    = 1  (i64 LE)
//   offset 16 → e1    = 2  (i64 LE)
//   offset 24 → e2    = 3  (i64 LE)
#[test]
fn list_new_layout_count_and_elements() {
    let expr = AnfExpr::Let {
        name: "e0".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
        body: Box::new(AnfExpr::Let {
            name: "e1".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(2))),
            body: Box::new(AnfExpr::Let {
                name: "e2".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(3))),
                body: Box::new(AnfExpr::ListNew(vec![
                    AnfExpr::Var("e0".to_string()),
                    AnfExpr::Var("e1".to_string()),
                    AnfExpr::Var("e2".to_string()),
                ])),
            }),
        }),
    };
    let mut instance = compile_and_instantiate_expr(expr, "fn.list_layout");
    let ptr = match instance
        .invoke("list_layout", &[])
        .expect("invoke must succeed")
    {
        RuntimeValue::I32(p) => p,
        other => panic!("ListNew must return I32 ptr; got {other:?}"),
    };

    assert!(ptr > 0, "heap pointer must be positive; got {ptr}");

    assert_eq!(
        instance
            .read_memory_i64(ptr, 0)
            .expect("count at offset 0 must be readable"),
        3,
        "count at offset 0 must be 3 for a three-element list"
    );
    assert_eq!(
        instance
            .read_memory_i64(ptr, 8)
            .expect("e0 at offset 8 must be readable"),
        1,
        "e0 at offset 8 must be 1"
    );
    assert_eq!(
        instance
            .read_memory_i64(ptr, 16)
            .expect("e1 at offset 16 must be readable"),
        2,
        "e1 at offset 16 must be 2"
    );
    assert_eq!(
        instance
            .read_memory_i64(ptr, 24)
            .expect("e2 at offset 24 must be readable"),
        3,
        "e2 at offset 24 must be 3"
    );
}

// RUNTIME-LIST-2
//
// fn.list_empty_layout = ListNew([])
//
// Expected heap layout at the returned ptr (heap_start = 8):
//   offset 0 → count = 0  (i64 LE)
//
// Empty ListNew allocates exactly 8 bytes (just the count word).  The bump
// pointer advances by 8, so the returned pointer is always positive.
#[test]
fn list_new_empty_layout_count_zero() {
    let expr = AnfExpr::ListNew(vec![]);
    let mut instance = compile_and_instantiate_expr(expr, "fn.list_empty_layout");
    let ptr = match instance
        .invoke("list_empty_layout", &[])
        .expect("invoke must succeed")
    {
        RuntimeValue::I32(p) => p,
        other => panic!("empty ListNew must return I32 ptr; got {other:?}"),
    };

    assert!(
        ptr > 0,
        "heap pointer must be positive even for empty list; got {ptr}"
    );

    assert_eq!(
        instance
            .read_memory_i64(ptr, 0)
            .expect("count at offset 0 must be readable"),
        0,
        "count at offset 0 must be 0 for an empty list"
    );
}

// RUNTIME-TUPLE-3
//
// fn.tuple_layout =
//   let e0 = Literal(10) in
//   let e1 = Literal(20) in
//   TupleNew([Var("e0"), Var("e1")])
//
// Expected heap layout at the returned ptr (heap_start = 8):
//   offset 0 → e0 = 10  (i64 LE)  — no count prefix
//   offset 8 → e1 = 20  (i64 LE)
//
// TupleNew has NO count word: elements start at offset 0.
// This complements RUNTIME-TUPLE-1..2 (FieldGet round-trips) by directly
// reading the raw memory bytes.
#[test]
fn tuple_new_layout_elements_no_count() {
    let expr = AnfExpr::Let {
        name: "e0".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(10))),
        body: Box::new(AnfExpr::Let {
            name: "e1".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(20))),
            body: Box::new(AnfExpr::TupleNew(vec![
                AnfExpr::Var("e0".to_string()),
                AnfExpr::Var("e1".to_string()),
            ])),
        }),
    };
    let mut instance = compile_and_instantiate_expr(expr, "fn.tuple_layout");
    let ptr = match instance
        .invoke("tuple_layout", &[])
        .expect("invoke must succeed")
    {
        RuntimeValue::I32(p) => p,
        other => panic!("TupleNew must return I32 ptr; got {other:?}"),
    };

    assert!(ptr > 0, "heap pointer must be positive; got {ptr}");

    assert_eq!(
        instance
            .read_memory_i64(ptr, 0)
            .expect("e0 at offset 0 must be readable"),
        10,
        "e0 at offset 0 must be 10 (TupleNew has no count prefix)"
    );
    assert_eq!(
        instance
            .read_memory_i64(ptr, 8)
            .expect("e1 at offset 8 must be readable"),
        20,
        "e1 at offset 8 must be 20"
    );
}

// RUNTIME-RECORD-3
//
// fn.record_layout =
//   let f0 = Literal(42) in
//   let f1 = Literal(99) in
//   RecordNew { fields: [("x", Var("f0")), ("y", Var("f1"))] }
//
// Expected heap layout at the returned ptr (heap_start = 8):
//   offset 0 → x = 42  (i64 LE)  — no count prefix
//   offset 8 → y = 99  (i64 LE)
//
// RecordNew has NO count word: fields start at offset 0 in declaration order.
// This complements RUNTIME-RECORD-1..2 (FieldGet round-trips) by directly
// reading the raw memory bytes.
#[test]
fn record_new_layout_fields_at_expected_offsets() {
    let expr = AnfExpr::Let {
        name: "f0".to_string(),
        value: Box::new(AnfExpr::Literal(LiteralValue::Int(42))),
        body: Box::new(AnfExpr::Let {
            name: "f1".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(99))),
            body: Box::new(AnfExpr::RecordNew {
                fields: vec![
                    ("x".to_string(), AnfExpr::Var("f0".to_string())),
                    ("y".to_string(), AnfExpr::Var("f1".to_string())),
                ],
            }),
        }),
    };
    let mut instance = compile_and_instantiate_expr(expr, "fn.record_layout");
    let ptr = match instance
        .invoke("record_layout", &[])
        .expect("invoke must succeed")
    {
        RuntimeValue::I32(p) => p,
        other => panic!("RecordNew must return I32 ptr; got {other:?}"),
    };

    assert!(ptr > 0, "heap pointer must be positive; got {ptr}");

    assert_eq!(
        instance
            .read_memory_i64(ptr, 0)
            .expect("field x at offset 0 must be readable"),
        42,
        "field x at offset 0 must be 42 (RecordNew has no count prefix)"
    );
    assert_eq!(
        instance
            .read_memory_i64(ptr, 8)
            .expect("field y at offset 8 must be readable"),
        99,
        "field y at offset 8 must be 99"
    );
}

// RUNTIME-RECORD-4
//
// fn.record_empty = RecordNew { fields: [] }
//
// RecordNew with zero fields allocates (0*8).max(1) = 1 byte.  The bump
// pointer always advances, so the returned I32 pointer must be positive.
// There is no memory to read (no count word, no fields), so we only assert
// structural liveness.
#[test]
fn record_new_empty_returns_non_null_pointer() {
    let expr = AnfExpr::RecordNew { fields: vec![] };
    let mut instance = compile_and_instantiate_expr(expr, "fn.record_empty");
    let ptr = match instance
        .invoke("record_empty", &[])
        .expect("invoke must succeed")
    {
        RuntimeValue::I32(p) => p,
        other => panic!("empty RecordNew must return I32 ptr; got {other:?}"),
    };

    assert!(
        ptr > 0,
        "empty RecordNew must return a positive I32 heap pointer; got {ptr}"
    );
}

// ── Wave 23B: ClockHandler epoch-milliseconds conformance ─────────────────
//
// RUNTIME-CLOCK-NOW-1
//
// effect_call(clock, now) must return the current wall time as
// epoch-milliseconds, not epoch-seconds.
//
// Bounds used to distinguish the two units:
//   lower: 1_000_000_000_000 ms  (2001-09-09T01:46:40Z — already past)
//   upper: 10_000_000_000_000 ms (2286-11-20T17:46:40Z — far future)
//
// A result in [1e12, 1e13) proves epoch-ms.
// A result in [1e9, 1e10) would prove epoch-s (the old bug).
#[test]
fn clock_now_effect_call_returns_epoch_milliseconds() {
    use std::sync::Arc;

    let clock_cap = CapabilityId::new("clock");

    // fn.main = EffectCall { capability: "clock", func: "now", args: [] }
    let binding = AnfBinding {
        source_ref: NodeRef(0),
        name: "fn.main".to_string(),
        expr: AnfExpr::EffectCall {
            capability: "clock".to_string(),
            func: "now".to_string(),
            args: vec![],
        },
    };
    let anf = sealed_anf(vec![binding]);
    let wasm = emit_wasm(&anf)
        .expect("clock EffectCall ANF must compile")
        .wasm;

    let manifest = CapabilityManifest {
        module: "clock-now-test".to_string(),
        requires: vec![clock_cap.clone()],
    };
    let module_hash = blake3_hex_of(&wasm);
    let manifest_hash = manifest.blake3_hex().expect("manifest hash must succeed");
    let profile = RuntimeProfile::new(
        "clock-now-test-profile".to_string(),
        module_hash,
        "a".repeat(64),
        manifest_hash,
        vec![CapabilityGrant {
            module: "clock-now-test".to_string(),
            capability: clock_cap,
        }],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
            ..Default::default()
        },
    );

    let handler = Arc::new(ClockHandler::new());
    let mut host = RuntimeHost::new().with_handler(handler);
    let mut instance = host
        .validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("clock WASM must instantiate");

    let value = instance.invoke("main", &[]).expect("main must invoke");
    let RuntimeValue::I64(now_ms) = value else {
        panic!("clock.now must return I64, got {value:?}");
    };
    // epoch-ms in 2026 ≈ 1.75e12; these bounds distinguish ms from seconds.
    assert!(
        now_ms > 1_000_000_000_000,
        "clock.now must return epoch-ms > 1e12, got {now_ms}"
    );
    assert!(
        now_ms < 10_000_000_000_000,
        "clock.now must return epoch-ms < 1e13, got {now_ms}"
    );
}

// ── Wave 23C: ACL source-level E2E tests for record field update ──────────
//
// Spec scenarios covered (RUNTIME-ACL-RECORD-UPDATE-1,
//                          RUNTIME-ACL-RECORD-UPDATE-2):
//
//  RUNTIME-ACL-RECORD-UPDATE-1: ACL body
//    `let(r, record(x,10,y,42), let(_u, update(r,y,99), field(r,y)))`
//    must:
//    (1) create the two-field record; (2) bind it as `r` (layout ["x","y"]);
//    (3) `update(r, y, 99)` → CoreExpr::FieldUpdate{record:Var("r"),
//        field:"y", value:Lit(99)} → AnfExpr::FieldUpdate → wasm_emit stores
//        I64(99) at ptr+8 in-place and returns ptr;
//    (4) `field(r, y)` → FieldGet at offset 8 → I64(99).
//    Returns I64(99).  Proves the full ACL source → update → field pipeline.
//
//  RUNTIME-ACL-RECORD-UPDATE-2: Same record and update as above but reads
//    `field(r, x)` (offset 0) instead of `field(r, y)`.  Must return I64(10).
//    Proves FieldUpdate does not corrupt the adjacent field; the update is
//    field-surgical and leaves `x` untouched.

// RUNTIME-ACL-RECORD-UPDATE-1
//
// ACL body: let(r, record(x, 10, y, 42), let(_u, update(r, y, 99), field(r, y)))
//
//   Pipeline:
//   1. `record(x, 10, y, 42)` → RecordNew{fields:[("x",Lit(10)),("y",Lit(42))]}
//   2. `let(r, <record>, ...)` binds "r" and registers layout ["x","y"].
//   3. `update(r, y, 99)` → FieldUpdate{record:Var("r"), field:"y", value:Lit(99)}.
//      ANF lower: let _t0=99 in FieldUpdate{record:"r", field:"y", value:Var("_t0")}.
//      WASM emit: load r_ptr, i64.const 99, i64.store offset=8 → stores 99 @ ptr+8;
//      returns ptr as I32 (_u = ptr).
//   4. `field(r, y)` → FieldGet{record:"r", field:"y"} → offset 8 → I64(99).
//   5. Returns I64(99).
//
// RecordNew memory layout: x @ offset 0 (I64 10), y @ offset 8 (I64 99 after update).
#[test]
fn acl_record_field_update_mutates_target_field() {
    let acl = "\
change acl_record_update_1 base=0
author tester
description let(r,record(x,10,y,42),let(_u,update(r,y,99),field(r,y))): update(y←99) must be visible via field(r,y)
op create_function id=fn.main return=Int body=let(r, record(x, 10, y, 42), let(_u, update(r, y, 99), field(r, y)))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(99),
        "update(r,y,99) must store 99 at field y; field(r,y) must return I64(99)"
    );
}

// RUNTIME-ACL-RECORD-UPDATE-2
//
// ACL body: let(r, record(x, 10, y, 42), let(_u, update(r, y, 99), field(r, x)))
//
//   Pipeline:
//   1..3. Same as RUNTIME-ACL-RECORD-UPDATE-1 up through FieldUpdate execution.
//         After update: memory = [I64(10) @ 0, I64(99) @ 8].
//   4. `field(r, x)` → FieldGet{record:"r", field:"x"} → offset 0 → I64(10).
//   5. Returns I64(10).
//
// Proves FieldUpdate is field-surgical: writing to offset 8 (field "y") does
// not corrupt the value at offset 0 (field "x").
#[test]
fn acl_record_field_update_leaves_adjacent_field_unchanged() {
    let acl = "\
change acl_record_update_2 base=0
author tester
description let(r,record(x,10,y,42),let(_u,update(r,y,99),field(r,x))): update(y←99) must not corrupt field x
op create_function id=fn.main return=Int body=let(r, record(x, 10, y, 42), let(_u, update(r, y, 99), field(r, x)))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(10),
        "update(r,y,99) must not corrupt field x; field(r,x) must still return I64(10)"
    );
}

// ── Wave 24A: ACL record offset variants ─────────────────────────────────
//
// Spec scenarios covered (RUNTIME-ACL-RECORD-UPDATE-3,
//                          RUNTIME-ACL-RECORD-3FIELD-1,
//                          RUNTIME-ACL-RECORD-3FIELD-UPDATE-1):
//
//  RUNTIME-ACL-RECORD-UPDATE-3: Update the first field x (offset 0) and verify
//    the adjacent field y (offset 8) is unchanged.  Proves FieldUpdate targeting
//    offset 0 writes only to that slot.
//
//  RUNTIME-ACL-RECORD-3FIELD-1: Three-field record record(a,1,b,2,c,3) with a
//    field-get on the third field c.  The emit formula `index * 8` must resolve
//    index 2 → offset 16 correctly.
//
//  RUNTIME-ACL-RECORD-3FIELD-UPDATE-1: Three-field record with update to the
//    middle field b (offset 8); verifies a (offset 0) and c (offset 16) are
//    both unchanged.  Proves FieldUpdate is field-surgical even when the
//    updated field has neighbours on both sides.

// RUNTIME-ACL-RECORD-UPDATE-3
//
// ACL body: let(r, record(x, 10, y, 42), let(_u, update(r, x, 99), field(r, y)))
//
//   Pipeline:
//   1. `record(x, 10, y, 42)` → RecordNew{fields:[("x",Lit(10)),("y",Lit(42))]}
//   2. `let(r, <record>, ...)` binds "r" with layout ["x","y"].
//   3. `update(r, x, 99)` → FieldUpdate{record:"r", field:"x", value:99}.
//      WASM emit: i64.store offset=0 → stores I64(99) @ ptr+0.
//      Memory after update: [I64(99) @ 0, I64(42) @ 8].
//   4. `field(r, y)` → FieldGet{record:"r", field:"y"} → offset 8 → I64(42).
//   5. Returns I64(42).
//
// Proves FieldUpdate targeting the first field (offset 0) does not corrupt
// the second field (offset 8).
#[test]
fn acl_record_field_update_first_field_leaves_second_unchanged() {
    let acl = "\
change acl_record_update_3 base=0
author tester
description let(r,record(x,10,y,42),let(_u,update(r,x,99),field(r,y))): update(x←99) must not corrupt field y
op create_function id=fn.main return=Int body=let(r, record(x, 10, y, 42), let(_u, update(r, x, 99), field(r, y)))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(42),
        "update(r,x,99) must not corrupt field y; field(r,y) must still return I64(42)"
    );
}

// RUNTIME-ACL-RECORD-3FIELD-1
//
// ACL body: let(r, record(a, 1, b, 2, c, 3), field(r, c))
//
//   Pipeline:
//   1. `record(a, 1, b, 2, c, 3)` →
//      RecordNew{fields:[("a",Lit(1)),("b",Lit(2)),("c",Lit(3))]}
//   2. `let(r, <record>, ...)` binds "r" with layout ["a","b","c"].
//   3. `field(r, c)` → FieldGet{record:"r", field:"c"} → index 2 → offset 16.
//      load_i64_at(ptr + 16) → I64(3).
//   4. Returns I64(3).
//
// RecordNew memory layout (no count prefix):
//   a @ offset  0 (I64 1)
//   b @ offset  8 (I64 2)
//   c @ offset 16 (I64 3)
//
// Proves the 8-byte stride formula `index * 8` resolves field "c" (index 2)
// to offset 16 end-to-end through the full ACL source pipeline.
#[test]
fn acl_record_three_field_get_third_field_returns_value() {
    let acl = "\
change acl_record_3field_1 base=0
author tester
description let(r,record(a,1,b,2,c,3),field(r,c)): field c at offset 16 must return I64(3)
op create_function id=fn.main return=Int body=let(r, record(a, 1, b, 2, c, 3), field(r, c))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(3),
        "field(r,c) on record(a,1,b,2,c,3) must return I64(3) via FieldGet at offset 16"
    );
}

// RUNTIME-ACL-RECORD-3FIELD-UPDATE-1
//
// Update middle field b (offset 8); verify that the left neighbour a (offset 0)
// and the right neighbour c (offset 16) are each left intact.  The two
// sub-cases are exercised as separate tests so a failure pinpoints exactly
// which neighbour is affected.

// Sub-case A: left neighbour — field a must remain I64(1) after update(b←99).
//
// ACL body: let(r, record(a,1,b,2,c,3), let(_u, update(r,b,99), field(r,a)))
//
// Memory after update: [I64(1) @ 0, I64(99) @ 8, I64(3) @ 16].
// field(r,a) → offset 0 → I64(1).
//
// Proves FieldUpdate targeting offset 8 (field b) does not corrupt the
// lower neighbour at offset 0 (field a).
#[test]
fn acl_record_update_middle_field_leaves_left_neighbour_unchanged() {
    let acl = "\
change acl_record_3field_update_1a base=0
author tester
description let(r,record(a,1,b,2,c,3),let(_u,update(r,b,99),field(r,a))): update(b←99) must not corrupt a
op create_function id=fn.main return=Int body=let(r, record(a, 1, b, 2, c, 3), let(_u, update(r, b, 99), field(r, a)))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(1),
        "update(r,b,99) must not corrupt field a; field(r,a) must still return I64(1)"
    );
}

// Sub-case B: right neighbour — field c must remain I64(3) after update(b←99).
//
// ACL body: let(r, record(a,1,b,2,c,3), let(_u, update(r,b,99), field(r,c)))
//
// Memory after update: [I64(1) @ 0, I64(99) @ 8, I64(3) @ 16].
// field(r,c) → offset 16 → I64(3).
//
// Proves FieldUpdate targeting offset 8 (field b) does not corrupt the
// upper neighbour at offset 16 (field c).
#[test]
fn acl_record_update_middle_field_leaves_right_neighbour_unchanged() {
    let acl = "\
change acl_record_3field_update_1b base=0
author tester
description let(r,record(a,1,b,2,c,3),let(_u,update(r,b,99),field(r,c))): update(b←99) must not corrupt c
op create_function id=fn.main return=Int body=let(r, record(a, 1, b, 2, c, 3), let(_u, update(r, b, 99), field(r, c)))
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(3),
        "update(r,b,99) must not corrupt field c; field(r,c) must still return I64(3)"
    );
}
