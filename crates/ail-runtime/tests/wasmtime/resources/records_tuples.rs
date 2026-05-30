use crate::helpers::*;

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
