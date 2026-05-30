use crate::helpers::*;

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
