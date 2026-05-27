use super::helpers::*;

#[test]
fn variant_match_single_binding_extracts_payload_and_uses_it() {
    let expr = make_variant_match_expr(
        "Ok",
        Some(21),
        vec![
            AnfMatchArm {
                pattern: "Ok(x)".to_string(),
                body: AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["x".to_string(), "x".to_string()],
                },
            },
            AnfMatchArm {
                pattern: "_".to_string(),
                body: AnfExpr::Literal(LiteralValue::Int(0)),
            },
        ],
    );
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(42),
        "Ok(x) arm must load payload 21 and compute x+x=42"
    );
}

// RUNTIME-VARIANT-MATCH-2
//
// VariantNew("None") → discriminant=0, no payload written.
// Match arm "None" fires: discriminant 0 == 0 ✓; no payload load attempted.
// Arm body returns literal 99.
#[test]
fn variant_match_tag_only_pattern_matches_none_variant() {
    let expr = make_variant_match_expr(
        "None",
        None,
        vec![
            AnfMatchArm {
                pattern: "None".to_string(),
                body: AnfExpr::Literal(LiteralValue::Int(99)),
            },
            AnfMatchArm {
                pattern: "_".to_string(),
                body: AnfExpr::Literal(LiteralValue::Int(0)),
            },
        ],
    );
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(99),
        "None tag-only pattern must match discriminant 0 and return 99"
    );
}

// RUNTIME-VARIANT-MATCH-3
//
// VariantNew("Err", payload=1) → discriminant=1.
// Match arm "Ok(x)" checks discriminant==0 → fails (1≠0); wildcard fires → 999.
// Proves the wrong-tag arm is skipped entirely.
#[test]
fn variant_match_wildcard_fires_on_wrong_tag() {
    let expr = make_variant_match_expr(
        "Err",
        Some(1),
        vec![
            AnfMatchArm {
                pattern: "Ok(x)".to_string(),
                body: AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["x".to_string(), "x".to_string()],
                },
            },
            AnfMatchArm {
                pattern: "_".to_string(),
                body: AnfExpr::Literal(LiteralValue::Int(999)),
            },
        ],
    );
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(999),
        "Err variant (discriminant=1) must skip Ok(x) arm and hit wildcard returning 999"
    );
}

// RUNTIME-VARIANT-MATCH-4
//
// VariantNew("Some", payload=21) → discriminant=1.
// Arms: "None" (discriminant=0) → 1 [skipped], "Some(x)" (discriminant=1) → x+x [matches!],
//       "_" → 999 [never reached].
// Proves ordering: the first arm is evaluated and rejected before the
// correct arm fires and extracts the payload.
#[test]
fn variant_match_ordering_skips_wrong_arms_and_matches_correct_one() {
    let expr = make_variant_match_expr(
        "Some",
        Some(21),
        vec![
            AnfMatchArm {
                pattern: "None".to_string(),
                body: AnfExpr::Literal(LiteralValue::Int(1)),
            },
            AnfMatchArm {
                pattern: "Some(x)".to_string(),
                body: AnfExpr::Call {
                    func: "+".to_string(),
                    args: vec!["x".to_string(), "x".to_string()],
                },
            },
            AnfMatchArm {
                pattern: "_".to_string(),
                body: AnfExpr::Literal(LiteralValue::Int(999)),
            },
        ],
    );
    assert_eq!(
        invoke_compiler_expr(expr, "fn.main"),
        RuntimeValue::I64(42),
        "Some(x) arm must match after skipping None; x+x with payload=21 must return 42"
    );
}

// ── Wave 19C / Wave 20A: ACL source-level E2E conformance — variant + match + string + while ──
//
// Spec scenarios covered (RUNTIME-ACL-SOME-1, RUNTIME-ACL-NONE-1,
// RUNTIME-ACL-OK-1, RUNTIME-ACL-ERR-1, RUNTIME-ACL-STRING-1,
// RUNTIME-ACL-WHILE-1, RUNTIME-ACL-WHILE-2,
// RUNTIME-ACL-WHILE-3, RUNTIME-ACL-WHILE-4):
//
//  RUNTIME-ACL-SOME-1: ACL body `match(some(42), Some(x), x, _, 0)` must
//    construct a Some(42) variant, enter the Some(x) arm, bind x=42, and
//    return I64(42).  Proves the full pipeline from ACL source through
//    VariantNew emission and constructor-pattern match dispatch.
//
//  RUNTIME-ACL-NONE-1: ACL body `match(none(), None, 99, _, 0)` must
//    construct a None variant (tag_id=0) and dispatch to the `None` arm
//    (tag-only, no payload binding), returning I64(99).  Proves tag-only
//    constructor patterns fire correctly.
//
//  RUNTIME-ACL-OK-1: ACL body `match(ok(7), Ok(v), v, Err(e), 0)` must
//    construct an Ok(7) variant and dispatch to the Ok(v) arm, returning I64(7).
//    Proves Ok/Err share the same well-known tag encoding as None/Some.
//
//  RUNTIME-ACL-ERR-1: ACL body `match(err(5), Ok(v), 0, Err(e), e)` must
//    construct an Err(5) variant, skip the Ok(v) arm (tag mismatch), dispatch
//    to Err(e), and return I64(5).  Proves the second arm fires correctly.
//
//  RUNTIME-ACL-STRING-1: ACL body `let(s, "hello", s)` must compile the
//    string literal "hello" and return it as a packed I64 where the upper
//    32 bits encode the string length (5).  Proves string literals survive the
//    ACL parse → expr_parser → lower → WASM emit pipeline without loss.
//
//  RUNTIME-ACL-WHILE-1: ACL body `let(flag, false, while(flag, 42))` must
//    enter the while loop, find the condition false, never execute the body,
//    and return I32(0) (unit).  Proves WhileLoop with a Var-condition at the
//    ACL level exits immediately and produces the unit sentinel.
//
//  RUNTIME-ACL-WHILE-2: A multi-let ACL body creates a cell, runs a while
//    loop that writes 1 to the cell and breaks, then reads the cell.  Must
//    return I64(1).  Proves the while body executes exactly once, CellSet
//    persists to linear memory, CellGet reads back the written value, and
//    break exits the loop.  All sub-expression arguments are pre-bound Vars
//    so that no atomized binding is lost through the lower_core_expr_to_anf_local
//    `_` fallthrough (documented gap: non-Var while-condition expressions).
//
//  RUNTIME-ACL-WHILE-3 (Wave 20A): ACL body uses `while(lt(x, 3), ...)` where
//    the condition is a computed `lt` call, not a pre-bound Var.  With the
//    lower_core_expr_to_anf_local fix (WhileLoop arm), the binding for the
//    computed condition is properly emitted as a Let before the WhileLoop.
//    x=0 → lt(0,3)=true → loop body runs, writes 99 to cell, breaks.
//    CellGet must return I64(99).  Without the fix the condition binding is
//    discarded, emit_condition_get falls back to I32Const(0), the loop never
//    runs, and CellGet returns I64(0).
//
//  RUNTIME-ACL-WHILE-4 (Wave 20A): ACL body uses `while(eq(cell_get(c), zero),
//    ...)` where the condition involves a CellGet call.  Exercises the WhileLoop
//    + CellGet atomization fix together.  c=0, zero=0 → eq(0,0)=true → loop
//    body runs, writes 7 to cell, breaks.  CellGet must return I64(7).

// RUNTIME-ACL-SOME-1
//
// ACL body: match(some(42), Some(x), x, _, 0)
//
//   Pipeline:
//   1. `some(42)` → CoreExpr::VariantNew{tag:"Some", payload:Literal(42)}
//   2. Lowered to: Let{anf_0=42, anf_1=VariantNew{tag:"Some",payload:Var(anf_0)},
//                      Match{scrutinee:anf_1, arms:[Some(x)→x, _→0]}}
//   3. WASM: alloc 16 bytes, store tag_id("Some")=1 at offset 0,
//      store I64(42) at offset 8; then match: tag==1 → bind x=payload → x=42.
//   4. Returns I64(42).
#[test]
fn acl_some_match_extracts_payload() {
    let acl = "\
change acl_some_1 base=0
author tester
description some/match round-trip: Some(x) arm extracts the i64 payload
op create_function id=fn.main return=Int body=match(some(42), Some(x), x, _, 0)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(42),
        "match(some(42), Some(x), x, _, 0) must return I64(42)"
    );
}

// RUNTIME-ACL-NONE-1
//
// ACL body: match(none(), None, 99, _, 0)
//
//   Pipeline:
//   1. `none()` → CoreExpr::VariantNew{tag:"None", payload:None}
//   2. Lowered to: Let{anf_0=VariantNew{tag:"None",payload:None},
//                      Match{scrutinee:anf_0, arms:[None→99, _→0]}}
//   3. WASM: alloc 16 bytes, store tag_id("None")=0 at offset 0;
//      match: tag==0 → no payload binding → body=99.
//   4. Returns I64(99).
//
// Well-known tag table: None=0, Ok=0, Some=1, Err=1.
#[test]
fn acl_none_match_fires_none_arm() {
    let acl = "\
change acl_none_1 base=0
author tester
description none/match: None tag-only arm fires, wildcard fallback returns 0
op create_function id=fn.main return=Int body=match(none(), None, 99, _, 0)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(99),
        "match(none(), None, 99, _, 0) must return I64(99)"
    );
}

// RUNTIME-ACL-OK-1
//
// ACL body: match(ok(7), Ok(v), v, Err(e), 0)
//
//   Pipeline:
//   1. `ok(7)` → VariantNew{tag:"Ok", payload:Literal(7)}, tag_id("Ok")=0
//   2. Match: Ok(v) arm → tag_id("Ok")=0 matches → bind v=7 → return v.
//   3. Err(e) arm is unreachable in this invocation.
//   4. Returns I64(7).
#[test]
fn acl_ok_match_extracts_ok_payload() {
    let acl = "\
change acl_ok_1 base=0
author tester
description ok/match round-trip: Ok(v) arm extracts the i64 payload
op create_function id=fn.main return=Int body=match(ok(7), Ok(v), v, Err(e), 0)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(7),
        "match(ok(7), Ok(v), v, Err(e), 0) must return I64(7)"
    );
}

// RUNTIME-ACL-ERR-1
//
// ACL body: match(err(5), Ok(v), 0, Err(e), e)
//
//   Pipeline:
//   1. `err(5)` → VariantNew{tag:"Err", payload:Literal(5)}, tag_id("Err")=1
//   2. Match: Ok(v) arm → tag_id("Ok")=0 ≠ 1 → skip.
//             Err(e) arm → tag_id("Err")=1 matches → bind e=5 → return e.
//   3. Returns I64(5).
//
// Proves the second match arm fires when the first arm's tag does not match.
#[test]
fn acl_err_match_fires_err_arm() {
    let acl = "\
change acl_err_1 base=0
author tester
description err/match: Err(e) arm fires when Ok(v) arm tag does not match
op create_function id=fn.main return=Int body=match(err(5), Ok(v), 0, Err(e), e)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(5),
        "match(err(5), Ok(v), 0, Err(e), e) must return I64(5)"
    );
}

// RUNTIME-ACL-STRING-1
//
// ACL body: let(s, "hello", s)
//
//   Pipeline:
//   1. `bare_value_end` preserves inner quotes: body_expr = `let(s, "hello", s)`.
//   2. expr_parser sees `"hello"` inside parse_args → Literal(Text("hello")).
//   3. WASM emit: Text literal → I64Const((len << 32) | ptr).
//      For "hello" (5 bytes): upper 32 bits = 5.
//   4. Returns I64(packed) with upper 32 bits = 5.
//
// The exact ptr (lower 32 bits) depends on the data segment and is not
// asserted — only the stable length field is checked.
#[test]
fn acl_string_literal_body_encodes_length_in_upper_bits() {
    let acl = r#"
change acl_string_1 base=0
author tester
description string literal body: "hello" must encode len=5 in upper I64 bits
op create_function id=fn.main return=Text body=let(s, "hello", s)
end
"#;
    let value = invoke_acl_export(acl, "main");
    let RuntimeValue::I64(packed) = value else {
        panic!("expected RuntimeValue::I64 for string body, got {value:?}");
    };
    let len = (packed as u64 >> 32) as u32;
    assert_eq!(
        len, 5,
        "string \"hello\" must encode length 5 in upper 32 bits of the packed I64; got len={len}"
    );
}

// RUNTIME-ACL-WHILE-1
//
// ACL body: let(flag, false, while(flag, 42))
//
//   Pipeline:
//   1. flag = I64(0) (false).
//   2. WhileLoop: emit_condition_get("flag") → I64(0); I64Const(0); I64Ne → I32(0);
//      I32Eqz → I32(1); BrIf(1) → branch taken (condition is zero).
//      Body (Literal(42)) is never reached.
//   3. WhileLoop pushes I32Const(0) → returns I32(0) (unit).
//
// Constraint: the while-condition must be a Var already in scope.
// If the condition expression is non-atomic (e.g. `while(lt(x,5), ...)`),
// the lower_core_expr_to_anf_local `_` fallthrough loses atomized bindings —
// see documented gap in Wave 19C session summary.

// ── Wave 24B: ACL source-level E2E tests for user-defined variant tags ────
//
// Spec scenarios covered (RUNTIME-ACL-VARIANT-USER-1..3):
//
//  RUNTIME-ACL-VARIANT-USER-1: ACL body
//    `match(variant(Status, 1), Status(x), x, _, 0)` must construct a
//    user-defined Status variant carrying payload 1, match the Status(x) arm,
//    bind x=1, and return I64(1).  Proves the full ACL source pipeline for
//    user-defined constructor tags with payload extraction: parse → VariantNew
//    → lower → wasm_emit → runtime dispatch.
//
//    Tag-assignment note: "Status" is not a well-known tag (None/Ok/Some/Err),
//    so assign_tag assigns it discriminant 2 on first encounter (user tags
//    start at 2 after the fix that reserves 0/1 for well-known tags).  The
//    constructor stores 2 at heap offset 0; the match arm checks offset 0 == 2
//    (cache hit) → fires; payload (1) is loaded from offset 8 and bound to x.
//    ANF ordering: VariantNew emit precedes Match emit via let-binding, so the tag cache is populated before arm walk.
//
//  RUNTIME-ACL-VARIANT-USER-2: Two user-defined tags Active and Inactive
//    dispatch to distinct arms.  Two functions in the same ACL module share
//    the same arm list `Active, 1, Inactive, 2, _, 0` but construct different
//    variants:
//    - fn.test_active  constructs variant(Active)  → Active arm fires  → 1
//    - fn.test_inactive constructs variant(Inactive) → Inactive arm fires → 2
//
//    Tag-assignment within each function's emit context:
//    - fn.test_active:   "Active" is first → id=0; arm "Active"→0 matches.
//    - fn.test_inactive: "Inactive" is first → id=0; arm "Active"→1, arm
//      "Inactive"→0 (from cache) → matches.
//
//    Proves that user-defined tag discriminants are assigned consistently
//    within a function context regardless of declaration order in the arm list.
//
//  RUNTIME-ACL-VARIANT-USER-3: A user-defined variant whose tag does not
//    match any named arm falls through to the wildcard.  ACL body
//    `match(variant(Pending), Active, 1, Inactive, 2, _, 99)`:
//    "Pending" → discriminant 0; arm "Active" → 1; arm "Inactive" → 2.
//    Neither named arm matches discriminant 0 → wildcard fires → 99.
//
//    Proves the wildcard fallback is reached when no constructor arm matches
//    the stored discriminant, mirroring RUNTIME-VARIANT-MATCH-3 at the
//    ACL source level without using well-known tags.

// RUNTIME-ACL-VARIANT-USER-1
//
// ACL body: match(variant(Status, 1), Status(x), x, _, 0)
//
//   Pipeline:
//   1. `variant(Status, 1)` → parse_variant_call →
//      CoreExpr::VariantNew { tag: "Status", payload: Some(Literal(1)) }
//   2. lower_to_anf → let _t0=1 in let _t1=VariantNew{tag:"Status",payload:_t0} in
//      Match{scrutinee:"_t1", arms:[Status(x)→Var("x"), _→Literal(0)]}
//   3. WASM emit VariantNew: assign_tag("Status")=2 (user tags start at 2;
//      no well-known tags present in this context);
//      alloc 16 bytes; store tag_id=2 at offset 0 (I32), store 1 at offset 8 (I64).
//   4. Match arm "Status(x)": parse_constructor_pattern → ("Status", Some("x"));
//      load I32 @ offset 0 = 2; assign_tag("Status")=2 (from cache);
//      I32Eq(2, 2) → true → bind x = I64 @ offset 8 = 1 → return Var("x") = 1.
//   5. Wildcard arm is dead code in this invocation.
//   6. Returns I64(1).
//
// Proves: ACL `variant` constructor form → user-defined VariantNew → match
// arm with payload binding → correct I64 return — end-to-end.
#[test]
fn acl_variant_user_1_status_match_extracts_payload() {
    let acl = "\
change acl_variant_user_1 base=0
author tester
description variant(Status, 1) matched by Status(x) must return I64(1)
op create_function id=fn.main return=Int body=match(variant(Status, 1), Status(x), x, _, 0)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(1),
        "match(variant(Status, 1), Status(x), x, _, 0) must return I64(1)"
    );
}

// RUNTIME-ACL-VARIANT-USER-2
//
// Two functions in the same ACL module, both with arms [Active→1, Inactive→2, _→0]:
//
//   fn.test_active:   body = match(variant(Active), Active, 1, Inactive, 2, _, 0)
//   fn.test_inactive: body = match(variant(Inactive), Active, 1, Inactive, 2, _, 0)
//
// fn.test_active emit context:
//   - VariantNew "Active"   → assign_tag("Active")   = 2  (first user tag; starts at 2)
//   - Match arm "Active"    → assign_tag("Active")   = 2  (cache hit) → 2==2 fires → 1
//
// fn.test_inactive emit context (fresh WasmCodegenCtx — build_code_section creates a fresh context; no shared tag cache):
//   - VariantNew "Inactive"  → assign_tag("Inactive")  = 2  (first user tag)
//   - Match arm "Active"     → assign_tag("Active")    = 3  (second user tag)
//   - Match arm "Inactive"   → assign_tag("Inactive")  = 2  (cache hit) → 2==2 fires → 2
//
// Proves:
// - fn.test_active  → I64(1): Active arm fires (2==2), Inactive arm is not reached.
// - fn.test_inactive → I64(2): Active arm fails (2≠3), Inactive arm fires (2==2).
//
// Each function gets a fresh WasmCodegenCtx (build_code_section creates a fresh
// context; no shared tag cache), so tag assignments in each function are
// independent and dispatch to distinct arms within their own emit context.
#[test]
fn acl_variant_user_2_active_inactive_dispatch_distinctly() {
    let acl = "\
change acl_variant_user_2 base=0
author tester
description Active and Inactive user tags must dispatch to distinct arms
op create_function id=fn.test_active return=Int body=match(variant(Active), Active, 1, Inactive, 2, _, 0)
op create_function id=fn.test_inactive return=Int body=match(variant(Inactive), Active, 1, Inactive, 2, _, 0)
end
";
    assert_eq!(
        invoke_acl_export(acl, "test_active"),
        RuntimeValue::I64(1),
        "variant(Active) must match Active arm and return I64(1)"
    );
    assert_eq!(
        invoke_acl_export(acl, "test_inactive"),
        RuntimeValue::I64(2),
        "variant(Inactive) must skip Active arm and match Inactive arm, returning I64(2)"
    );
}

// RUNTIME-ACL-VARIANT-USER-3
//
// ACL body: match(variant(Pending), Active, 1, Inactive, 2, _, 99)
//
//   Pipeline:
//   1. `variant(Pending)` → VariantNew { tag: "Pending", payload: None }
//   2. lower_to_anf → let _t0=VariantNew{tag:"Pending",payload:None} in
//      Match{scrutinee:"_t0", arms:[Active→1, Inactive→2, _→99]}
//   3. WASM emit VariantNew: assign_tag("Pending")=2 (first user tag; starts at 2);
//      alloc 16 bytes; store tag_id=2 at offset 0 (I32); no payload written.
//   4. Match arm "Active":   assign_tag("Active")  =3; I32Eq(2,3)=false → Else.
//      Match arm "Inactive": assign_tag("Inactive")=4; I32Eq(2,4)=false → Else.
//      Wildcard "_": unconditionally emits body → Literal(99).
//   5. Returns I64(99).
//
// Proves: when no named constructor arm matches the stored discriminant, the
// wildcard arm fires and the fallback value is returned — mirroring
// RUNTIME-VARIANT-MATCH-3 at the ACL source level, using only user-defined
// tags (no None/Ok/Some/Err mixing).
#[test]
fn acl_variant_user_3_mismatch_falls_to_wildcard() {
    let acl = "\
change acl_variant_user_3 base=0
author tester
description variant(Pending) with no matching arm must fall to wildcard returning 99
op create_function id=fn.main return=Int body=match(variant(Pending), Active, 1, Inactive, 2, _, 99)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(99),
        "variant(Pending) must skip Active and Inactive arms and fall to wildcard returning I64(99)"
    );
}

// ── Wave 25C: ACL E2E tests — well-known vs user-defined variant tag collision ──
//
// Spec scenarios covered (RUNTIME-ACL-VARIANT-COLLISION-1..3):
//
//  RUNTIME-ACL-VARIANT-COLLISION-1: user tag `Active` must NOT match the
//    `None` arm.  Before the fix (`next_variant_tag: 0`), `assign_tag("Active")`
//    would return 0 — the same discriminant as well-known `None` — causing
//    the `None` arm to fire erroneously.  After the fix (`next_variant_tag: 2`),
//    user tags start at 2 and can never collide with reserved IDs 0 (`None`/`Ok`)
//    or 1 (`Some`/`Err`).  The wildcard arm must fire, returning I64(99).
//
//  RUNTIME-ACL-VARIANT-COLLISION-2: `variant(None)` and `none()` must produce
//    the same discriminant (0).  Both forms resolve through `well_known_variant_tag`,
//    bypassing the user-tag counter entirely.  Two functions in the same module —
//    one using `none()`, the other using `variant(None)` — must both match the
//    `None` arm and return I64(42).
//
//  RUNTIME-ACL-VARIANT-COLLISION-3: well-known `None`=0, `Ok`=0, `Some`=1,
//    and `Err`=1 must remain stable after the user-tag-reservation fix.  The
//    `next_variant_tag` counter starts at 2 and can only grow, so it stays at
//    ≥2 regardless of which well-known tags are resolved; well-known tags bypass
//    the counter entirely (via `well_known_variant_tag`).  The `max` guard in
//    `assign_tag` is only relevant for hypothetical future well-known IDs ≥2 —
//    it ensures user tags never alias any such ID should one be introduced.
//    `none()` and `ok(7)` must match discriminant 0; `some(42)` and `err(5)`
//    must match discriminant 1.

// RUNTIME-ACL-VARIANT-COLLISION-1
//
// ACL body: match(variant(Active), None, 1, _, 99)
//
//   Pre-fix behaviour (WRONG — exposes the bug):
//     assign_tag("Active") = 0  (next_variant_tag started at 0)
//     assign_tag("None")   = 0  (well-known)
//     I32Eq(0, 0) → true → None arm fires → returns I64(1)  ← COLLISION
//
//   Post-fix behaviour (CORRECT):
//     assign_tag("Active") = 2  (user tags start at 2)
//     assign_tag("None")   = 0  (well-known; counter unchanged)
//     I32Eq(2, 0) → false → Else → wildcard fires → returns I64(99)
//
// Proves: user-defined tags are guaranteed never to alias well-known IDs 0/1.
#[test]
fn acl_variant_collision_1_user_active_does_not_match_none_arm() {
    let acl = "\
change acl_variant_collision_1 base=0
author tester
description user tag Active must not collide with well-known None discriminant 0
op create_function id=fn.main return=Int body=match(variant(Active), None, 1, _, 99)
end
";
    assert_eq!(
        invoke_acl_export(acl, "main"),
        RuntimeValue::I64(99),
        "user tag Active (discriminant 2) must not match None arm (discriminant 0); wildcard must fire returning I64(99)"
    );
}

// RUNTIME-ACL-VARIANT-COLLISION-2
//
// Two functions, same module:
//   fn.via_none_call:    body = match(none(),         None, 42, _, 0)
//   fn.via_variant_none: body = match(variant(None),  None, 42, _, 0)
//
//   Pipeline for fn.via_none_call:
//     `none()` → VariantNew{tag:"None", payload:None}
//     assign_tag("None") → well_known_variant_tag("None") = 0; store tag 0.
//     Match "None" → assign_tag("None") = 0 (cache); I32Eq(0,0) → fires → 42.
//
//   Pipeline for fn.via_variant_none:
//     `variant(None)` → parse_variant_call → VariantNew{tag:"None", payload:None}
//     assign_tag("None") → well_known_variant_tag("None") = 0; store tag 0.
//     Match "None" → assign_tag("None") = 0 (cache); I32Eq(0,0) → fires → 42.
//
// Both forms resolve "None" through well_known_variant_tag, so they produce
// identical discriminant 0 — the user-tag counter is never involved.
#[test]
fn acl_variant_collision_2_variant_none_and_none_call_same_discriminant() {
    let acl = "\
change acl_variant_collision_2 base=0
author tester
description variant(None) and none() must both produce discriminant 0 and match the None arm
op create_function id=fn.via_none_call return=Int body=match(none(), None, 42, _, 0)
op create_function id=fn.via_variant_none return=Int body=match(variant(None), None, 42, _, 0)
end
";
    assert_eq!(
        invoke_acl_export(acl, "via_none_call"),
        RuntimeValue::I64(42),
        "none() must produce discriminant 0 and match the None arm, returning I64(42)"
    );
    assert_eq!(
        invoke_acl_export(acl, "via_variant_none"),
        RuntimeValue::I64(42),
        "variant(None) must produce discriminant 0 — same as none() — and match the None arm, returning I64(42)"
    );
}

// RUNTIME-ACL-VARIANT-COLLISION-3
//
// Four functions proving well-known discriminants are stable after the fix:
//   fn.none_stable: match(none(),   None,    10, _, 0) → I64(10)
//   fn.some_stable: match(some(42), Some(x), x,  _, 0) → I64(42)
//   fn.ok_stable:   match(ok(7),    Ok(v),   v,  _, 0) → I64(7)
//   fn.err_stable:  match(err(5),   Err(e),  e,  _, 0) → I64(5)
//
//   Pipeline for fn.none_stable / fn.ok_stable (discriminant 0):
//     `none()` / `ok(7)` → VariantNew → assign_tag("None"/"Ok") = 0 (well-known).
//     Store tag 0.  Match arm tag-check: 0==0 → fires.  Returns I64(10) / I64(7).
//
//   Pipeline for fn.some_stable / fn.err_stable (discriminant 1):
//     `some(42)` / `err(5)` → VariantNew → assign_tag("Some"/"Err") = 1 (well-known).
//     Store tag 1, payload 42/5.  Match arm tag-check: 1==1 → fires → binds payload.
//     Returns I64(42) / I64(5).
//
//   Causality: the counter stays at ≥2 because it starts at 2 and can only
//   grow — it is never decremented.  Well-known tags bypass the counter
//   entirely (via well_known_variant_tag), so resolving None/Ok/Some/Err
//   never touches `next_variant_tag`.  The max-guard in assign_tag only
//   matters for hypothetical future well-known IDs ≥2: if such an ID were
//   introduced, the guard bumps the counter past it so user tags still cannot
//   alias it.  None=0, Ok=0, Some=1, Err=1 are unaffected.
#[test]
fn acl_variant_collision_3_well_known_none_some_ok_err_remain_stable() {
    let acl = "\
change acl_variant_collision_3 base=0
author tester
description well-known None=0, Ok=0, Some=1, Err=1 remain stable; not displaced by user-tag reservation
op create_function id=fn.none_stable return=Int body=match(none(), None, 10, _, 0)
op create_function id=fn.some_stable return=Int body=match(some(42), Some(x), x, _, 0)
op create_function id=fn.ok_stable return=Int body=match(ok(7), Ok(v), v, _, 0)
op create_function id=fn.err_stable return=Int body=match(err(5), Err(e), e, _, 0)
end
";
    assert_eq!(
        invoke_acl_export(acl, "none_stable"),
        RuntimeValue::I64(10),
        "none() must still match None arm (discriminant 0 unchanged) and return I64(10)"
    );
    assert_eq!(
        invoke_acl_export(acl, "some_stable"),
        RuntimeValue::I64(42),
        "some(42) must still match Some(x) arm (discriminant 1 unchanged), bind x=42, return I64(42)"
    );
    assert_eq!(
        invoke_acl_export(acl, "ok_stable"),
        RuntimeValue::I64(7),
        "ok(7) must still match Ok(v) arm (discriminant 0 unchanged), bind v=7, return I64(7)"
    );
    assert_eq!(
        invoke_acl_export(acl, "err_stable"),
        RuntimeValue::I64(5),
        "err(5) must still match Err(e) arm (discriminant 1 unchanged), bind e=5, return I64(5)"
    );
}
