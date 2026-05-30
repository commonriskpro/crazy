use crate::helpers::*;

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
