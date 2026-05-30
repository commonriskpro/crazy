use super::*;

// ── G23: Lowering tests for new concurrency + cell primitives ─────────

// TaskAwait: Var task → no synthetic bindings, atomic name preserved.
#[test]
fn lower_task_await_var_is_preserved() {
    use crate::core_ir::CoreExpr;
    let expr = CoreExpr::TaskAwait {
        task: Box::new(CoreExpr::Var("task_0".to_string())),
    };
    let (result, out) = lower_single(&expr);
    assert!(
        out.is_empty(),
        "Var task must not produce synthetic bindings"
    );
    match result {
        crate::anf::AnfExpr::TaskAwait { task } => {
            assert_eq!(task, "task_0");
        }
        other => panic!("expected TaskAwait, got {other:?}"),
    }
}

// TRIANGULATE: TaskAwait with non-Var task atomizes it.
#[test]
fn lower_task_await_complex_task_is_atomized() {
    use crate::core_ir::CoreExpr;
    let expr = CoreExpr::TaskAwait {
        task: Box::new(CoreExpr::Call {
            func: "fn.spawn_work".to_string(),
            args: vec![],
        }),
    };
    let (result, out) = lower_single(&expr);
    assert!(
        !out.is_empty(),
        "non-Var task must produce a synthetic binding"
    );
    match result {
        crate::anf::AnfExpr::TaskAwait { task } => {
            assert!(task.starts_with("anf_"), "task must be synthetic: {task}");
        }
        other => panic!("expected TaskAwait, got {other:?}"),
    }
}

// TaskCancel: Var task → no synthetic bindings.
#[test]
fn lower_task_cancel_var_is_preserved() {
    use crate::core_ir::CoreExpr;
    let expr = CoreExpr::TaskCancel {
        task: Box::new(CoreExpr::Var("t".to_string())),
    };
    let (result, out) = lower_single(&expr);
    assert!(out.is_empty());
    match result {
        crate::anf::AnfExpr::TaskCancel { task } => {
            assert_eq!(task, "t");
        }
        other => panic!("expected TaskCancel, got {other:?}"),
    }
}

// TaskGroup: body is lowered recursively.
#[test]
fn lower_task_group_body_is_lowered() {
    use crate::core_ir::CoreExpr;
    let expr = CoreExpr::TaskGroup {
        body: Box::new(CoreExpr::Var("spawner".to_string())),
    };
    let (result, out) = lower_single(&expr);
    assert!(
        out.is_empty(),
        "Var body must not produce synthetic bindings"
    );
    match result {
        crate::anf::AnfExpr::TaskGroup { body } => {
            assert_eq!(*body, crate::anf::AnfExpr::Var("spawner".to_string()));
        }
        other => panic!("expected TaskGroup, got {other:?}"),
    }
}

// ChannelNew unbounded: no sub-expressions to lower.
#[test]
fn lower_channel_new_unbounded() {
    use crate::core_ir::CoreExpr;
    let expr = CoreExpr::ChannelNew { capacity: None };
    let (result, out) = lower_single(&expr);
    assert!(
        out.is_empty(),
        "ChannelNew must produce no synthetic bindings"
    );
    match result {
        crate::anf::AnfExpr::ChannelNew { capacity } => {
            assert!(capacity.is_none());
        }
        other => panic!("expected ChannelNew, got {other:?}"),
    }
}

// TRIANGULATE: ChannelNew bounded preserves capacity.
#[test]
fn lower_channel_new_bounded_preserves_capacity() {
    use crate::core_ir::CoreExpr;
    let expr = CoreExpr::ChannelNew { capacity: Some(64) };
    let (result, out) = lower_single(&expr);
    assert!(out.is_empty());
    match result {
        crate::anf::AnfExpr::ChannelNew { capacity } => {
            assert_eq!(capacity, Some(64));
        }
        other => panic!("expected ChannelNew, got {other:?}"),
    }
}

// Select: Var channel → no synthetic bindings; clause fields preserved.
#[test]
fn lower_select_var_channel_is_preserved() {
    use crate::core_ir::{CoreExpr, SelectClause};
    let expr = CoreExpr::Select {
        branches: vec![SelectClause {
            channel: Box::new(CoreExpr::Var("inbox".to_string())),
            binding: "item".to_string(),
            body: CoreExpr::Var("item".to_string()),
        }],
    };
    let (result, out) = lower_single(&expr);
    assert!(
        out.is_empty(),
        "Var channel must not produce synthetic bindings"
    );
    match result {
        crate::anf::AnfExpr::Select { branches } => {
            assert_eq!(branches.len(), 1);
            assert_eq!(branches[0].channel, "inbox");
            assert_eq!(branches[0].binding, "item");
            assert_eq!(
                branches[0].body,
                crate::anf::AnfExpr::Var("item".to_string())
            );
        }
        other => panic!("expected Select, got {other:?}"),
    }
}

// TRIANGULATE: Select with non-Var channel atomizes it.
#[test]
fn lower_select_complex_channel_is_atomized() {
    use crate::core_ir::{CoreExpr, LiteralValue, SelectClause};
    let expr = CoreExpr::Select {
        branches: vec![SelectClause {
            channel: Box::new(CoreExpr::Call {
                func: "fn.get_channel".to_string(),
                args: vec![],
            }),
            binding: "v".to_string(),
            body: CoreExpr::Literal(LiteralValue::Unit),
        }],
    };
    let (result, out) = lower_single(&expr);
    assert!(
        !out.is_empty(),
        "non-Var channel must produce a synthetic binding"
    );
    match result {
        crate::anf::AnfExpr::Select { branches } => {
            assert!(branches[0].channel.starts_with("anf_"));
        }
        other => panic!("expected Select, got {other:?}"),
    }
}

// Timeout: Var duration → no synthetic bindings; body lowered recursively.
#[test]
fn lower_timeout_var_duration_is_preserved() {
    use crate::core_ir::{CoreExpr, LiteralValue};
    let expr = CoreExpr::Timeout {
        duration: Box::new(CoreExpr::Var("ms".to_string())),
        body: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
    };
    let (result, out) = lower_single(&expr);
    assert!(
        out.is_empty(),
        "Var duration must not produce synthetic bindings"
    );
    match result {
        crate::anf::AnfExpr::Timeout { duration, body } => {
            assert_eq!(duration, "ms");
            assert_eq!(*body, crate::anf::AnfExpr::Literal(LiteralValue::Unit));
        }
        other => panic!("expected Timeout, got {other:?}"),
    }
}

// TRIANGULATE: Timeout with non-Var duration atomizes it.
#[test]
fn lower_timeout_complex_duration_is_atomized() {
    use crate::core_ir::{CoreExpr, LiteralValue};
    let expr = CoreExpr::Timeout {
        duration: Box::new(CoreExpr::Literal(LiteralValue::Int(5000))),
        body: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
    };
    let (result, out) = lower_single(&expr);
    assert!(
        !out.is_empty(),
        "Literal duration must produce a synthetic binding"
    );
    match result {
        crate::anf::AnfExpr::Timeout { duration, .. } => {
            assert!(
                duration.starts_with("anf_"),
                "duration must be synthetic: {duration}"
            );
        }
        other => panic!("expected Timeout, got {other:?}"),
    }
}

// CellNew: Var init → no synthetic bindings.
#[test]
fn lower_cell_new_var_init_is_preserved() {
    use crate::core_ir::CoreExpr;
    let expr = CoreExpr::CellNew {
        init: Box::new(CoreExpr::Var("zero".to_string())),
    };
    let (result, out) = lower_single(&expr);
    assert!(
        out.is_empty(),
        "Var init must not produce synthetic bindings"
    );
    match result {
        crate::anf::AnfExpr::CellNew { init } => {
            assert_eq!(init, "zero");
        }
        other => panic!("expected CellNew, got {other:?}"),
    }
}

// TRIANGULATE: CellNew with Literal init atomizes it.
#[test]
fn lower_cell_new_literal_init_is_atomized() {
    use crate::core_ir::{CoreExpr, LiteralValue};
    let expr = CoreExpr::CellNew {
        init: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
    };
    let (result, out) = lower_single(&expr);
    assert!(
        !out.is_empty(),
        "Literal init must produce a synthetic binding"
    );
    match result {
        crate::anf::AnfExpr::CellNew { init } => {
            assert!(init.starts_with("anf_"), "init must be synthetic: {init}");
        }
        other => panic!("expected CellNew, got {other:?}"),
    }
}

// CellGet: Var cell → no synthetic bindings.
#[test]
fn lower_cell_get_var_cell_is_preserved() {
    use crate::core_ir::CoreExpr;
    let expr = CoreExpr::CellGet {
        cell: Box::new(CoreExpr::Var("counter".to_string())),
    };
    let (result, out) = lower_single(&expr);
    assert!(out.is_empty());
    match result {
        crate::anf::AnfExpr::CellGet { cell } => {
            assert_eq!(cell, "counter");
        }
        other => panic!("expected CellGet, got {other:?}"),
    }
}

// CellSet: both Var cell and Var value → no synthetic bindings.
#[test]
fn lower_cell_set_var_operands_are_preserved() {
    use crate::core_ir::CoreExpr;
    let expr = CoreExpr::CellSet {
        cell: Box::new(CoreExpr::Var("c".to_string())),
        value: Box::new(CoreExpr::Var("v".to_string())),
    };
    let (result, out) = lower_single(&expr);
    assert!(
        out.is_empty(),
        "Var operands must not produce synthetic bindings"
    );
    match result {
        crate::anf::AnfExpr::CellSet { cell, value } => {
            assert_eq!(cell, "c");
            assert_eq!(value, "v");
        }
        other => panic!("expected CellSet, got {other:?}"),
    }
}

// TRIANGULATE: CellSet with non-Var cell and non-Var value atomizes both.
#[test]
fn lower_cell_set_literal_operands_are_atomized() {
    use crate::core_ir::{CoreExpr, LiteralValue};
    let expr = CoreExpr::CellSet {
        cell: Box::new(CoreExpr::Call {
            func: "fn.get_cell".to_string(),
            args: vec![],
        }),
        value: Box::new(CoreExpr::Literal(LiteralValue::Int(42))),
    };
    let (result, out) = lower_single(&expr);
    assert_eq!(
        out.len(),
        2,
        "two non-Var operands must produce two synthetic bindings"
    );
    match result {
        crate::anf::AnfExpr::CellSet { cell, value } => {
            assert!(cell.starts_with("anf_"), "cell must be synthetic: {cell}");
            assert!(
                value.starts_with("anf_"),
                "value must be synthetic: {value}"
            );
        }
        other => panic!("expected CellSet, got {other:?}"),
    }
}
