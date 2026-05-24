// ── ail-compiler::anf_lowering_effects ───────────────────────────────────
//
// G3 / G20 R2: ANF lowering tests for effect-flavoured expressions.
//
// Spec scenarios covered:
//   R2-S5..S7   — Short-circuit And / Or
//   R2-S8..S9   — EffectCall lowering and atomization
//   R2-S10      — Dispatch lowering
//   R2-S11      — TaskSpawn lowering
//   R2-S12..S13 — ChannelSend / ChannelReceive lowering
//   R2-S14..S15 — RuntimeCheck lowering and atomization
//   R2-S16..S18 — ResourceAcquire / ResourceRelease lowering and atomization
//   R2-S22..S27 — CBOR round-trips for the above variants
//   R2-S28      — AnfIr schema_version survives CBOR
//   R2-S29      — AnfIr source_map entries match bindings count

mod anf_lowering_helpers;
use anf_lowering_helpers::core_ir_with_expr;

use ail_compiler::hash::stable_cbor_bytes;
use ail_compiler::lower::lower_core_expr_to_anf;
use ail_compiler::{AnfExpr, CoreExpr, LiteralValue, lower_to_anf};
use ail_core::semantic_graph::NodeRef;

// ── G20 R2: short-circuit lowering ────────────────────────────────────────

// R2-S5: CoreExpr::And lowers to AnfExpr::ShortCircuitAnd.
// Left is atomized; right is a nested AnfExpr (lazy evaluation).
#[test]
fn and_lowers_to_short_circuit_and() {
    let expr = CoreExpr::And {
        left: Box::new(CoreExpr::Var("a".to_string())),
        right: Box::new(CoreExpr::Var("b".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(
        out.is_empty(),
        "Var left must not produce synthetic bindings"
    );
    match result {
        AnfExpr::ShortCircuitAnd { left, right } => {
            assert_eq!(left, "a");
            assert_eq!(*right, AnfExpr::Var("b".to_string()));
        }
        other => panic!("expected ShortCircuitAnd, got {other:?}"),
    }
}

// R2-S6: CoreExpr::Or lowers to AnfExpr::ShortCircuitOr.
#[test]
fn or_lowers_to_short_circuit_or() {
    let expr = CoreExpr::Or {
        left: Box::new(CoreExpr::Var("x".to_string())),
        right: Box::new(CoreExpr::Var("y".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::ShortCircuitOr { left, right } => {
            assert_eq!(left, "x");
            assert_eq!(*right, AnfExpr::Var("y".to_string()));
        }
        other => panic!("expected ShortCircuitOr, got {other:?}"),
    }
}

// R2-S7: And with non-Var left → left is let-bound (atomized).
#[test]
fn and_with_complex_left_is_atomized() {
    let expr = CoreExpr::And {
        left: Box::new(CoreExpr::Literal(LiteralValue::Bool(true))),
        right: Box::new(CoreExpr::Var("b".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(out.len(), 1, "Literal left must be let-bound");
    match result {
        AnfExpr::ShortCircuitAnd { left, .. } => {
            assert!(left.starts_with("anf_"), "left must be synthetic: {left}");
        }
        other => panic!("expected ShortCircuitAnd, got {other:?}"),
    }
}

// ── G20 R2: EffectCall lowering ───────────────────────────────────────────

// R2-S8: CoreExpr::EffectCall lowers to AnfExpr::EffectCall with atomized args.
#[test]
fn effect_call_lowers_correctly() {
    let expr = CoreExpr::EffectCall {
        capability: "database".to_string(),
        func: "read".to_string(),
        args: vec![CoreExpr::Var("cart_id".to_string())],
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty(), "Var arg must not produce bindings");
    match result {
        AnfExpr::EffectCall {
            capability,
            func,
            args,
        } => {
            assert_eq!(capability, "database");
            assert_eq!(func, "read");
            assert_eq!(args, vec!["cart_id"]);
        }
        other => panic!("expected EffectCall, got {other:?}"),
    }
}

// R2-S9: EffectCall with non-Var arg atomizes it.
#[test]
fn effect_call_atomizes_non_var_args() {
    let expr = CoreExpr::EffectCall {
        capability: "payment".to_string(),
        func: "charge".to_string(),
        args: vec![CoreExpr::Literal(LiteralValue::Int(100))],
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(
        out.len(),
        1,
        "Literal arg must produce one synthetic binding"
    );
    match result {
        AnfExpr::EffectCall { args, .. } => {
            assert!(
                args[0].starts_with("anf_"),
                "arg must be synthetic: {}",
                args[0]
            );
        }
        other => panic!("expected EffectCall, got {other:?}"),
    }
}

// ── G20 R2: Dispatch lowering ─────────────────────────────────────────────

// R2-S10: CoreExpr::Dispatch lowers to AnfExpr::Dispatch.
#[test]
fn dispatch_lowers_correctly() {
    let expr = CoreExpr::Dispatch {
        handler: "PaymentProvider".to_string(),
        method: "charge".to_string(),
        args: vec![CoreExpr::Var("amount".to_string())],
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::Dispatch {
            handler,
            method,
            args,
        } => {
            assert_eq!(handler, "PaymentProvider");
            assert_eq!(method, "charge");
            assert_eq!(args, vec!["amount"]);
        }
        other => panic!("expected Dispatch, got {other:?}"),
    }
}

// ── G20 R2: TaskSpawn lowering ────────────────────────────────────────────

// R2-S11: CoreExpr::TaskSpawn lowers to AnfExpr::TaskSpawn.
#[test]
fn task_spawn_lowers_correctly() {
    let expr = CoreExpr::TaskSpawn {
        func: "worker.process".to_string(),
        args: vec![CoreExpr::Var("payload".to_string())],
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::TaskSpawn { func, args } => {
            assert_eq!(func, "worker.process");
            assert_eq!(args, vec!["payload"]);
        }
        other => panic!("expected TaskSpawn, got {other:?}"),
    }
}

// ── G20 R2: ChannelSend / ChannelReceive lowering ─────────────────────────

// R2-S12: CoreExpr::ChannelSend lowers to AnfExpr::ChannelSend (both atomic).
#[test]
fn channel_send_lowers_correctly() {
    let expr = CoreExpr::ChannelSend {
        channel: Box::new(CoreExpr::Var("ch".to_string())),
        value: Box::new(CoreExpr::Var("msg".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::ChannelSend { channel, value } => {
            assert_eq!(channel, "ch");
            assert_eq!(value, "msg");
        }
        other => panic!("expected ChannelSend, got {other:?}"),
    }
}

// R2-S13: CoreExpr::ChannelReceive lowers to AnfExpr::ChannelReceive.
#[test]
fn channel_recv_lowers_correctly() {
    let expr = CoreExpr::ChannelReceive {
        channel: Box::new(CoreExpr::Var("ch".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::ChannelReceive { channel } => {
            assert_eq!(channel, "ch");
        }
        other => panic!("expected ChannelReceive, got {other:?}"),
    }
}

// ── G20 R2: RuntimeCheck lowering ─────────────────────────────────────────

// R2-S14: CoreExpr::RuntimeCheck lowers to AnfExpr::RuntimeCheck.
// Contract checks MUST survive lowering.
#[test]
fn runtime_check_lowers_correctly() {
    let expr = CoreExpr::RuntimeCheck {
        check_ref: "contract.balance_non_negative".to_string(),
        cond: Box::new(CoreExpr::Var("is_valid".to_string())),
        msg: "balance must be non-negative".to_string(),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(
        out.is_empty(),
        "Var cond must not produce synthetic bindings"
    );
    match result {
        AnfExpr::RuntimeCheck {
            check_ref,
            cond,
            msg,
        } => {
            assert_eq!(check_ref, "contract.balance_non_negative");
            assert_eq!(cond, "is_valid");
            assert_eq!(msg, "balance must be non-negative");
        }
        other => panic!("expected RuntimeCheck, got {other:?}"),
    }
}

// R2-S15: RuntimeCheck with non-Var cond atomizes it.
#[test]
fn runtime_check_atomizes_non_var_cond() {
    let expr = CoreExpr::RuntimeCheck {
        check_ref: "contract.positive".to_string(),
        cond: Box::new(CoreExpr::Literal(LiteralValue::Bool(true))),
        msg: "must be positive".to_string(),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(out.len(), 1, "Literal cond must be let-bound");
    match result {
        AnfExpr::RuntimeCheck { cond, .. } => {
            assert!(cond.starts_with("anf_"), "cond must be synthetic: {cond}");
        }
        other => panic!("expected RuntimeCheck, got {other:?}"),
    }
}

// ── G20 R2: Resource acquire/release ordering ─────────────────────────────

// R2-S16: CoreExpr::ResourceAcquire lowers to AnfExpr::ResourceAcquire.
#[test]
fn resource_acquire_lowers_correctly() {
    let expr = CoreExpr::ResourceAcquire {
        resource: "db.connection".to_string(),
        args: vec![CoreExpr::Var("conn_str".to_string())],
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::ResourceAcquire { resource, args } => {
            assert_eq!(resource, "db.connection");
            assert_eq!(args, vec!["conn_str"]);
        }
        other => panic!("expected ResourceAcquire, got {other:?}"),
    }
}

// R2-S17: CoreExpr::ResourceRelease lowers to AnfExpr::ResourceRelease.
#[test]
fn resource_release_lowers_correctly() {
    let expr = CoreExpr::ResourceRelease {
        handle: Box::new(CoreExpr::Var("conn".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::ResourceRelease { handle } => {
            assert_eq!(handle, "conn");
        }
        other => panic!("expected ResourceRelease, got {other:?}"),
    }
}

// R2-S18: ResourceRelease atomizes non-Var handle.
#[test]
fn resource_release_atomizes_non_var_handle() {
    // Non-Var handle: a Call that returns a handle
    let expr = CoreExpr::ResourceRelease {
        handle: Box::new(CoreExpr::Call {
            func: "db.get_handle".to_string(),
            args: vec![],
        }),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(out.len(), 1, "Call handle must be let-bound");
    match result {
        AnfExpr::ResourceRelease { handle } => {
            assert!(
                handle.starts_with("anf_"),
                "handle must be synthetic: {handle}"
            );
        }
        other => panic!("expected ResourceRelease, got {other:?}"),
    }
}

// ── G20 R2: CBOR round-trips for new AnfExpr variants ────────────────────

// R2-S22: AnfExpr::ShortCircuitAnd CBOR round-trip.
#[test]
fn short_circuit_and_cbor_round_trip() {
    let expr = AnfExpr::ShortCircuitAnd {
        left: "a".to_string(),
        right: Box::new(AnfExpr::Var("b".to_string())),
    };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// R2-S23: AnfExpr::EffectCall CBOR round-trip.
#[test]
fn effect_call_cbor_round_trip() {
    let expr = AnfExpr::EffectCall {
        capability: "db".to_string(),
        func: "read".to_string(),
        args: vec!["cart_id".to_string()],
    };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// R2-S24: AnfExpr::RuntimeCheck CBOR round-trip.
#[test]
fn runtime_check_cbor_round_trip() {
    let expr = AnfExpr::RuntimeCheck {
        check_ref: "contract.positive".to_string(),
        cond: "is_valid".to_string(),
        msg: "must be positive".to_string(),
    };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// R2-S25: AnfExpr::ResourceAcquire CBOR round-trip.
#[test]
fn resource_acquire_cbor_round_trip() {
    let expr = AnfExpr::ResourceAcquire {
        resource: "db.conn".to_string(),
        args: vec!["conn_str".to_string()],
    };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// R2-S26: AnfExpr::TaskSpawn CBOR round-trip.
#[test]
fn task_spawn_cbor_round_trip() {
    let expr = AnfExpr::TaskSpawn {
        func: "worker.process".to_string(),
        args: vec!["payload".to_string()],
    };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// R2-S27: AnfExpr::Dispatch CBOR round-trip.
#[test]
fn dispatch_cbor_round_trip() {
    let expr = AnfExpr::Dispatch {
        handler: "PaymentProvider".to_string(),
        method: "charge".to_string(),
        args: vec!["amount".to_string()],
    };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// R2-S28: AnfIr schema_version is 1 and survives CBOR round-trip.
#[test]
fn anf_ir_schema_version_survives_cbor() {
    let anf = lower_to_anf(&core_ir_with_expr(
        NodeRef(0),
        "fn_test",
        CoreExpr::Literal(LiteralValue::Unit),
    ))
    .unwrap();
    assert_eq!(anf.schema_version, 1);

    let bytes = stable_cbor_bytes(&anf).unwrap();
    let decoded: ail_compiler::AnfIr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded.schema_version, 1);
}

// R2-S29: AnfIr source_map entries match bindings count and node_ids.
#[test]
fn anf_ir_source_map_matches_bindings() {
    let anf = lower_to_anf(&core_ir_with_expr(
        NodeRef(3),
        "fn_mapped",
        CoreExpr::Var("x".to_string()),
    ))
    .unwrap();
    // Every binding must have a corresponding source map entry.
    assert_eq!(anf.source_map.entries.len(), anf.bindings.len());
    // The last entry's node_id must match the root binding's source_ref.
    let last_entry = anf.source_map.entries.last().unwrap();
    let last_binding = anf.bindings.last().unwrap();
    assert_eq!(last_entry.node_id, last_binding.source_ref);
}
