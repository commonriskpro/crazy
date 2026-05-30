use crate::helpers::*;

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
