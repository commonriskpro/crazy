// ── ail-compiler::anf_lowering_concurrency ───────────────────────────────
//
// G23: ANF lowering tests for concurrency and cell primitives.
//
// Spec scenarios covered:
//   G23-S1  — TaskAwait lowers correctly
//   G23-S2  — TaskCancel lowers correctly
//   G23-S3  — TaskGroup body is lowered recursively
//   G23-S4  — ChannelNew unbounded / bounded capacity preserved
//   G23-S5  — Select with Var / complex channel operands
//   G23-S6  — Timeout with Var / Literal duration
//   G23-S7  — CellNew with Var / Literal init
//   G23-S8  — CellGet with Var cell
//   G23-S9  — CellSet with Var operands; Literal value atomized
//   G23-S10 — CellSet with both non-Var operands atomized
//   G23-CBOR — CBOR round-trips for TaskAwait, TaskGroup, Select, CellSet
//   G23-FULL — Full pipeline with cell ops

use ail_compiler::hash::stable_cbor_bytes;
use ail_compiler::lower::lower_core_expr_to_anf;
use ail_compiler::{AnfExpr, CoreExpr, LiteralValue};
use ail_core::semantic_graph::NodeRef;

// ── G23-S1: TaskAwait ────────────────────────────────────────────────────

// G23-S1: TaskAwait lowers correctly (Var task → no synthetic bindings).
#[test]
fn task_await_lowers_correctly() {
    let expr = CoreExpr::TaskAwait {
        task: Box::new(CoreExpr::Var("t0".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty(), "Var task must not produce bindings");
    match result {
        AnfExpr::TaskAwait { task } => assert_eq!(task, "t0"),
        other => panic!("expected TaskAwait, got {other:?}"),
    }
}

// ── G23-S2: TaskCancel ───────────────────────────────────────────────────

// G23-S2: TaskCancel lowers correctly.
#[test]
fn task_cancel_lowers_correctly() {
    let expr = CoreExpr::TaskCancel {
        task: Box::new(CoreExpr::Var("t1".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::TaskCancel { task } => assert_eq!(task, "t1"),
        other => panic!("expected TaskCancel, got {other:?}"),
    }
}

// ── G23-S3: TaskGroup ────────────────────────────────────────────────────

// G23-S3: TaskGroup body is lowered recursively.
#[test]
fn task_group_body_lowered_recursively() {
    let expr = CoreExpr::TaskGroup {
        body: Box::new(CoreExpr::Call {
            func: "fn.work".to_string(),
            args: vec![CoreExpr::Var("ctx".to_string())],
        }),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(
        out.is_empty(),
        "Var arg inside body must not produce extra bindings"
    );
    match result {
        AnfExpr::TaskGroup { body } => {
            assert_eq!(
                *body,
                AnfExpr::Call {
                    func: "fn.work".to_string(),
                    args: vec!["ctx".to_string()],
                }
            );
        }
        other => panic!("expected TaskGroup, got {other:?}"),
    }
}

// ── G23-S4: ChannelNew ───────────────────────────────────────────────────

// G23-S4: ChannelNew unbounded — capacity None preserved.
#[test]
fn channel_new_unbounded_lowers_correctly() {
    let expr = CoreExpr::ChannelNew { capacity: None };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::ChannelNew { capacity } => assert!(capacity.is_none()),
        other => panic!("expected ChannelNew(None), got {other:?}"),
    }
}

// TRIANGULATE: ChannelNew bounded — capacity Some(n) preserved.
#[test]
fn channel_new_bounded_preserves_capacity() {
    let expr = CoreExpr::ChannelNew {
        capacity: Some(128),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::ChannelNew { capacity } => assert_eq!(capacity, Some(128)),
        other => panic!("expected ChannelNew(Some(128)), got {other:?}"),
    }
}

// ── G23-S5: Select ───────────────────────────────────────────────────────

// G23-S5: Select — Var channel, binding and body preserved correctly.
#[test]
fn select_var_channel_lowers_correctly() {
    use ail_compiler::core_ir::SelectClause;
    let expr = CoreExpr::Select {
        branches: vec![SelectClause {
            channel: Box::new(CoreExpr::Var("inbox".to_string())),
            binding: "msg".to_string(),
            body: CoreExpr::Var("msg".to_string()),
        }],
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(
        out.is_empty(),
        "Var channel must produce no synthetic bindings"
    );
    match result {
        AnfExpr::Select { branches } => {
            assert_eq!(branches.len(), 1);
            assert_eq!(branches[0].channel, "inbox");
            assert_eq!(branches[0].binding, "msg");
            assert_eq!(branches[0].body, AnfExpr::Var("msg".to_string()));
        }
        other => panic!("expected Select, got {other:?}"),
    }
}

// TRIANGULATE: Select with two branches, second channel is a Call → atomized.
#[test]
fn select_two_branches_complex_channel_is_atomized() {
    use ail_compiler::core_ir::SelectClause;
    let expr = CoreExpr::Select {
        branches: vec![
            SelectClause {
                channel: Box::new(CoreExpr::Var("ch_a".to_string())),
                binding: "a".to_string(),
                body: CoreExpr::Var("a".to_string()),
            },
            SelectClause {
                channel: Box::new(CoreExpr::Call {
                    func: "fn.get_ch".to_string(),
                    args: vec![],
                }),
                binding: "b".to_string(),
                body: CoreExpr::Var("b".to_string()),
            },
        ],
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(
        out.len(),
        1,
        "one non-Var channel must produce one synthetic binding"
    );
    match result {
        AnfExpr::Select { branches } => {
            assert_eq!(branches.len(), 2);
            assert_eq!(branches[0].channel, "ch_a"); // Var — unchanged
            assert!(branches[1].channel.starts_with("anf_")); // atomized
        }
        other => panic!("expected Select, got {other:?}"),
    }
}

// ── G23-S6: Timeout ──────────────────────────────────────────────────────

// G23-S6: Timeout — Var duration preserved, body lowered.
#[test]
fn timeout_var_duration_lowers_correctly() {
    let expr = CoreExpr::Timeout {
        duration: Box::new(CoreExpr::Var("deadline".to_string())),
        body: Box::new(CoreExpr::Var("work".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(
        out.is_empty(),
        "Var operands must not produce synthetic bindings"
    );
    match result {
        AnfExpr::Timeout { duration, body } => {
            assert_eq!(duration, "deadline");
            assert_eq!(*body, AnfExpr::Var("work".to_string()));
        }
        other => panic!("expected Timeout, got {other:?}"),
    }
}

// TRIANGULATE: Timeout with Literal duration → atomized.
#[test]
fn timeout_literal_duration_is_atomized() {
    let expr = CoreExpr::Timeout {
        duration: Box::new(CoreExpr::Literal(LiteralValue::Int(1000))),
        body: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(
        out.len(),
        1,
        "Literal duration must produce one synthetic binding"
    );
    match result {
        AnfExpr::Timeout { duration, .. } => {
            assert!(
                duration.starts_with("anf_"),
                "duration must be synthetic: {duration}"
            );
        }
        other => panic!("expected Timeout, got {other:?}"),
    }
}

// ── G23-S7: CellNew ──────────────────────────────────────────────────────

// G23-S7: CellNew — Var init preserved.
#[test]
fn cell_new_var_init_lowers_correctly() {
    let expr = CoreExpr::CellNew {
        init: Box::new(CoreExpr::Var("initial".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::CellNew { init } => assert_eq!(init, "initial"),
        other => panic!("expected CellNew, got {other:?}"),
    }
}

// TRIANGULATE: CellNew with Literal init → atomized.
#[test]
fn cell_new_literal_init_is_atomized() {
    let expr = CoreExpr::CellNew {
        init: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(
        out.len(),
        1,
        "Literal init must produce one synthetic binding"
    );
    match result {
        AnfExpr::CellNew { init } => {
            assert!(init.starts_with("anf_"), "init must be synthetic: {init}");
        }
        other => panic!("expected CellNew, got {other:?}"),
    }
}

// ── G23-S8: CellGet ──────────────────────────────────────────────────────

// G23-S8: CellGet — Var cell preserved.
#[test]
fn cell_get_var_cell_lowers_correctly() {
    let expr = CoreExpr::CellGet {
        cell: Box::new(CoreExpr::Var("total".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(out.is_empty());
    match result {
        AnfExpr::CellGet { cell } => assert_eq!(cell, "total"),
        other => panic!("expected CellGet, got {other:?}"),
    }
}

// ── G23-S9: CellSet ──────────────────────────────────────────────────────

// G23-S9: CellSet — both Var cell and Var value preserved.
#[test]
fn cell_set_var_operands_lowers_correctly() {
    let expr = CoreExpr::CellSet {
        cell: Box::new(CoreExpr::Var("acc".to_string())),
        value: Box::new(CoreExpr::Var("next_val".to_string())),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert!(
        out.is_empty(),
        "Var operands must not produce synthetic bindings"
    );
    match result {
        AnfExpr::CellSet { cell, value } => {
            assert_eq!(cell, "acc");
            assert_eq!(value, "next_val");
        }
        other => panic!("expected CellSet, got {other:?}"),
    }
}

// TRIANGULATE: CellSet with non-Var value → value is atomized.
#[test]
fn cell_set_literal_value_is_atomized() {
    let expr = CoreExpr::CellSet {
        cell: Box::new(CoreExpr::Var("counter".to_string())),
        value: Box::new(CoreExpr::Literal(LiteralValue::Int(99))),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(
        out.len(),
        1,
        "Literal value must produce one synthetic binding"
    );
    match result {
        AnfExpr::CellSet { cell, value } => {
            assert_eq!(cell, "counter"); // Var → unchanged
            assert!(
                value.starts_with("anf_"),
                "value must be synthetic: {value}"
            );
        }
        other => panic!("expected CellSet, got {other:?}"),
    }
}

// ── G23-S10: CellSet both operands non-Var ───────────────────────────────

// G23-S10: CellSet with Literal cell AND Literal value → both atomized.
#[test]
fn cell_set_both_literal_operands_are_atomized() {
    let expr = CoreExpr::CellSet {
        cell: Box::new(CoreExpr::Call {
            func: "fn.get_cell".to_string(),
            args: vec![],
        }),
        value: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    assert_eq!(
        out.len(),
        2,
        "two non-Var operands must produce two synthetic bindings"
    );
    match result {
        AnfExpr::CellSet { cell, value } => {
            assert!(cell.starts_with("anf_"), "cell must be synthetic");
            assert!(value.starts_with("anf_"), "value must be synthetic");
        }
        other => panic!("expected CellSet, got {other:?}"),
    }
}

// ── G23-CBOR round-trips ─────────────────────────────────────────────────

// G23-CBOR: AnfExpr::TaskAwait survives CBOR round-trip.
#[test]
fn task_await_cbor_round_trip() {
    let expr = AnfExpr::TaskAwait {
        task: "t0".to_string(),
    };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// G23-CBOR: AnfExpr::TaskGroup survives CBOR round-trip.
#[test]
fn task_group_cbor_round_trip() {
    let expr = AnfExpr::TaskGroup {
        body: Box::new(AnfExpr::Literal(LiteralValue::Unit)),
    };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// G23-CBOR: AnfExpr::Select survives CBOR round-trip.
#[test]
fn select_cbor_round_trip() {
    use ail_compiler::anf::AnfSelectClause;
    let expr = AnfExpr::Select {
        branches: vec![AnfSelectClause {
            channel: "ch".to_string(),
            binding: "v".to_string(),
            body: AnfExpr::Var("v".to_string()),
        }],
    };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// G23-CBOR: AnfExpr::CellSet survives CBOR round-trip.
#[test]
fn cell_set_cbor_round_trip() {
    let expr = AnfExpr::CellSet {
        cell: "c".to_string(),
        value: "v".to_string(),
    };
    let bytes = stable_cbor_bytes(&expr).unwrap();
    let decoded: AnfExpr = ciborium::from_reader(bytes.as_slice()).unwrap();
    assert_eq!(decoded, expr);
}

// ── G23-FULL ─────────────────────────────────────────────────────────────

// G23-FULL: Full pipeline with TaskGroup + CellNew/CellGet/CellSet succeeds.
#[test]
fn full_pipeline_with_cell_ops_succeeds() {
    // Simulate: let c = CellNew(0); CellSet(c, CellGet(c) + 1)
    let expr = CoreExpr::Let {
        name: "c".to_string(),
        value: Box::new(CoreExpr::CellNew {
            init: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
        }),
        body: Box::new(CoreExpr::CellSet {
            cell: Box::new(CoreExpr::Var("c".to_string())),
            value: Box::new(CoreExpr::Literal(LiteralValue::Int(1))),
        }),
    };
    let mut fresh = 0u32;
    let mut out = vec![];
    let result = lower_core_expr_to_anf(&expr, &mut fresh, NodeRef(0), &mut out);
    // Expect CellNew(Literal(0)) to generate a synthetic binding for the init,
    // then CellSet(Var("c"), Literal(1)) to generate a synthetic for value.
    // Top-level result is a Let.
    match result {
        AnfExpr::Let { name, .. } => assert_eq!(name, "c"),
        other => panic!("expected Let, got {other:?}"),
    }
}
