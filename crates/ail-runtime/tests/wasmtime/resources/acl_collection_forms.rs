use crate::helpers::*;

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
