use super::helpers::*;

#[test]
fn all_core_expr_variants_are_constructible() {
    let _literal = CoreExpr::Literal(LiteralValue::Int(42));
    let _var = CoreExpr::Var("x".to_string());
    let _let = CoreExpr::Let {
        name: "y".to_string(),
        value: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
        body: Box::new(CoreExpr::Var("y".to_string())),
    };
    let _if = CoreExpr::If {
        cond: Box::new(CoreExpr::Literal(LiteralValue::Bool(true))),
        then_: Box::new(CoreExpr::Literal(LiteralValue::Int(1))),
        else_: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
    };
    let _match = CoreExpr::Match {
        scrutinee: Box::new(CoreExpr::Var("v".to_string())),
        arms: vec![MatchArm {
            pattern: "Some(x)".to_string(),
            body: CoreExpr::Var("x".to_string()),
        }],
    };
    let _call = CoreExpr::Call {
        func: "fn.add".to_string(),
        args: vec![
            CoreExpr::Var("a".to_string()),
            CoreExpr::Var("b".to_string()),
        ],
    };
    let _lambda = CoreExpr::Lambda {
        params: vec!["x".to_string()],
        body: Box::new(CoreExpr::Var("x".to_string())),
    };
    let _record = CoreExpr::RecordNew {
        fields: vec![(
            "amount".to_string(),
            CoreExpr::Literal(LiteralValue::Int(10)),
        )],
    };
    let _field_get = CoreExpr::FieldGet {
        record: Box::new(CoreExpr::Var("order".to_string())),
        field: "total".to_string(),
    };
    let _field_update = CoreExpr::FieldUpdate {
        record: Box::new(CoreExpr::Var("order".to_string())),
        field: "status".to_string(),
        value: Box::new(CoreExpr::Literal(LiteralValue::Text("Paid".to_string()))),
    };
    let _tuple = CoreExpr::TupleNew(vec![
        CoreExpr::Literal(LiteralValue::Int(1)),
        CoreExpr::Literal(LiteralValue::Bool(false)),
    ]);
    let _variant = CoreExpr::VariantNew {
        tag: "Ok".to_string(),
        payload: Some(Box::new(CoreExpr::Literal(LiteralValue::Unit))),
    };
    let _list = CoreExpr::ListNew(vec![CoreExpr::Literal(LiteralValue::Int(1))]);
    let _loop = CoreExpr::Loop {
        body: Box::new(CoreExpr::Break {
            value: Box::new(CoreExpr::Literal(LiteralValue::Int(10))),
        }),
        termination: None,
    };
    let _break = CoreExpr::Break {
        value: Box::new(CoreExpr::Literal(LiteralValue::Int(1))),
    };
    let _continue = CoreExpr::Continue;
    let _while_loop = CoreExpr::WhileLoop {
        cond: Box::new(CoreExpr::Literal(LiteralValue::Bool(false))),
        body: Box::new(CoreExpr::Continue),
        termination: None,
    };
    let _placeholder = CoreExpr::Placeholder;
    // All constructed without panic — test passes.
}

// S4: CoreNode with CoreType payload round-trips through CBOR.

#[test]
fn literal_value_variants_eq() {
    assert_eq!(LiteralValue::Bool(true), LiteralValue::Bool(true));
    assert_ne!(LiteralValue::Bool(true), LiteralValue::Bool(false));
    assert_eq!(LiteralValue::Int(42), LiteralValue::Int(42));
    assert_eq!(LiteralValue::Float(1.0), LiteralValue::Float(1.0));
    assert_eq!(
        LiteralValue::Text("hello".to_string()),
        LiteralValue::Text("hello".to_string())
    );
    assert_eq!(LiteralValue::Unit, LiteralValue::Unit);
}

// G23: all new concurrency + cell CoreExpr variants are constructible.
#[test]
fn all_new_concurrency_cell_core_expr_variants_are_constructible() {
    let _task_await = CoreExpr::TaskAwait {
        task: Box::new(CoreExpr::Var("task_handle".to_string())),
    };
    let _task_cancel = CoreExpr::TaskCancel {
        task: Box::new(CoreExpr::Var("task_handle".to_string())),
    };
    let _task_group = CoreExpr::TaskGroup {
        body: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
    };
    let _channel_new_unbounded = CoreExpr::ChannelNew { capacity: None };
    let _channel_new_bounded = CoreExpr::ChannelNew { capacity: Some(16) };
    let _select = CoreExpr::Select {
        branches: vec![SelectClause {
            channel: Box::new(CoreExpr::Var("ch".to_string())),
            binding: "msg".to_string(),
            body: CoreExpr::Var("msg".to_string()),
        }],
    };
    let _timeout = CoreExpr::Timeout {
        duration: Box::new(CoreExpr::Var("dur_ms".to_string())),
        body: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
    };
    let _cell_new = CoreExpr::CellNew {
        init: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
    };
    let _cell_get = CoreExpr::CellGet {
        cell: Box::new(CoreExpr::Var("counter".to_string())),
    };
    let _cell_set = CoreExpr::CellSet {
        cell: Box::new(CoreExpr::Var("counter".to_string())),
        value: Box::new(CoreExpr::Literal(LiteralValue::Int(1))),
    };
    // All constructed without panic — test passes.
}

// TRIANGULATE: SelectClause is constructible and fields are accessible.
#[test]
fn select_clause_is_constructible_with_correct_fields() {
    let clause = SelectClause {
        channel: Box::new(CoreExpr::Var("inbox".to_string())),
        binding: "item".to_string(),
        body: CoreExpr::Var("item".to_string()),
    };
    assert_eq!(clause.binding, "item");
    assert_eq!(*clause.channel, CoreExpr::Var("inbox".to_string()));
    assert_eq!(clause.body, CoreExpr::Var("item".to_string()));
}

// G23: new concurrency variants round-trip through CBOR.

#[test]
fn match_arm_is_constructible() {
    let arm = MatchArm {
        pattern: "None".to_string(),
        body: CoreExpr::Placeholder,
    };
    assert_eq!(arm.pattern, "None");
    assert_eq!(arm.body, CoreExpr::Placeholder);
}

// ── Task C1 (RED): LoopTermination on Loop/WhileLoop ─────────────────

// S-C1a: Loop with termination=Some(Proven) round-trips through CBOR.
#[test]
fn loop_with_proven_termination_cbor_round_trip() {
    let expr = CoreExpr::Loop {
        body: Box::new(CoreExpr::Break {
            value: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
        }),
        termination: Some(LoopTermination::Proven),
    };
    let bytes = stable_cbor_bytes(&expr).expect("encode");
    let decoded: CoreExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(
        decoded, expr,
        "Loop with Proven termination must survive CBOR round-trip"
    );
    if let CoreExpr::Loop { termination, .. } = &decoded {
        assert_eq!(termination.as_ref(), Some(&LoopTermination::Proven));
    } else {
        panic!("expected Loop variant");
    }
}

// S-C1b: Loop with termination=None is backward-compatible.
// A loop without termination must produce the same bytes as before this change
// (serde skips None via skip_serializing_if).
#[test]
fn loop_without_termination_is_backward_compat() {
    let expr_with_none = CoreExpr::Loop {
        body: Box::new(CoreExpr::Continue),
        termination: None,
    };
    // The legacy form (before termination field) is equivalent to termination: None.
    // Verify round-trip preserves None.
    let bytes = stable_cbor_bytes(&expr_with_none).expect("encode");
    let decoded: CoreExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, expr_with_none);
    if let CoreExpr::Loop { termination, .. } = decoded {
        assert!(
            termination.is_none(),
            "termination must be None after round-trip"
        );
    }
}

// S-C1c: WhileLoop with termination=Some(Bounded) round-trips.
// Triangulation: WhileLoop termination works independently from Loop.
#[test]
fn while_loop_with_bounded_termination_cbor_round_trip() {
    let expr = CoreExpr::WhileLoop {
        cond: Box::new(CoreExpr::Literal(LiteralValue::Bool(true))),
        body: Box::new(CoreExpr::Continue),
        termination: Some(LoopTermination::Bounded),
    };
    let bytes = stable_cbor_bytes(&expr).expect("encode");
    let decoded: CoreExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(
        decoded, expr,
        "WhileLoop with Bounded termination must round-trip"
    );
}

// S-C1d: All LoopTermination variants are constructible.
#[test]
fn all_loop_termination_variants_are_constructible() {
    let _proven = LoopTermination::Proven;
    let _bounded = LoopTermination::Bounded;
    let _assumed = LoopTermination::Assumed;
    let _unverified = LoopTermination::Unverified;
    // All constructed without panic — test passes.
}

// ── Task B1 (RED): parameterized CoreType variants ────────────────────

// S-B1a: List(Box<CoreType::Int>) round-trips through CBOR.

#[test]
fn new_core_expr_variants_are_constructible() {
    let _for_each = CoreExpr::ForEach {
        binding: "item".to_string(),
        collection: Box::new(CoreExpr::Var("cart.items".to_string())),
        body: Box::new(CoreExpr::Placeholder),
    };
    let _fold = CoreExpr::Fold {
        init: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
        list: Box::new(CoreExpr::Var("items".to_string())),
        func: Box::new(CoreExpr::Var("add".to_string())),
    };
    let _return_expr = CoreExpr::Return {
        value: Box::new(CoreExpr::Literal(LiteralValue::Int(42))),
    };
    let _map_new = CoreExpr::MapNew {
        entries: vec![(
            CoreExpr::Literal(LiteralValue::Text("key".to_string())),
            CoreExpr::Literal(LiteralValue::Int(1)),
        )],
    };
    let _set_new = CoreExpr::SetNew {
        elements: vec![
            CoreExpr::Literal(LiteralValue::Int(1)),
            CoreExpr::Literal(LiteralValue::Int(2)),
        ],
    };
    let _index_get = CoreExpr::IndexGet {
        collection: Box::new(CoreExpr::Var("arr".to_string())),
        index: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
    };
    let _boundary_call = CoreExpr::BoundaryCall {
        boundary: "payments.stripe".to_string(),
        func: "charge".to_string(),
        args: vec![CoreExpr::Var("order".to_string())],
    };
    let _assume = CoreExpr::Assume {
        predicate: "x > 0".to_string(),
        reason: "validated at entry point".to_string(),
    };
    let _abort = CoreExpr::Abort {
        message: "unreachable: invalid state".to_string(),
    };
    // All constructed without panic — test passes.
}

// S-A3b: ForEach fields are accessible and correct after construction.
// Verifies the binding, collection, and body fields are correctly stored.
#[test]
fn for_each_fields_are_accessible() {
    let expr = CoreExpr::ForEach {
        binding: "item".to_string(),
        collection: Box::new(CoreExpr::Var("cart.items".to_string())),
        body: Box::new(CoreExpr::EffectCall {
            capability: "db".to_string(),
            func: "save".to_string(),
            args: vec![CoreExpr::Var("item".to_string())],
        }),
    };
    if let CoreExpr::ForEach {
        binding,
        collection,
        body,
    } = &expr
    {
        assert_eq!(binding, "item");
        assert_eq!(**collection, CoreExpr::Var("cart.items".to_string()));
        assert!(matches!(**body, CoreExpr::EffectCall { .. }));
    } else {
        panic!("expected ForEach variant");
    }
}

// S-A3c: BoundaryCall carries the boundary trust identifier.
#[test]
fn boundary_call_carries_trust_identifier() {
    let expr = CoreExpr::BoundaryCall {
        boundary: "payments.stripe".to_string(),
        func: "charge".to_string(),
        args: vec![],
    };
    if let CoreExpr::BoundaryCall {
        boundary,
        func,
        args,
    } = &expr
    {
        assert_eq!(boundary, "payments.stripe");
        assert_eq!(func, "charge");
        assert!(args.is_empty());
    } else {
        panic!("expected BoundaryCall variant");
    }
}

// S-A3d: Assume and Abort CBOR round-trips.
// Triangulation: two different structural variants with string fields.
#[test]
fn assume_and_abort_cbor_round_trip() {
    let assume_expr = CoreExpr::Assume {
        predicate: "balance >= 0".to_string(),
        reason: "invariant at domain boundary".to_string(),
    };
    let abort_expr = CoreExpr::Abort {
        message: "impossible branch reached".to_string(),
    };
    for expr in &[assume_expr, abort_expr] {
        let bytes = stable_cbor_bytes(expr).expect("encode");
        let decoded: CoreExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(&decoded, expr, "must survive CBOR round-trip");
    }
}

// ── Task A1 (RED): CoreType::Dyn and CoreExpr::DynCall ───────────────
// Tests written BEFORE the variants exist — compilation failure is the RED gate.

// A1-1: CoreType::Dyn carries the interface name and is constructible.

#[test]
fn dyn_call_core_expr_construction_and_fields() {
    let expr = CoreExpr::DynCall {
        interface: "Repository<User>".to_string(),
        method: "get".to_string(),
        args: vec![CoreExpr::Var("id".to_string())],
    };
    if let CoreExpr::DynCall {
        interface,
        method,
        args,
    } = &expr
    {
        assert_eq!(interface, "Repository<User>");
        assert_eq!(method, "get");
        assert_eq!(args.len(), 1);
        assert_eq!(args[0], CoreExpr::Var("id".to_string()));
    } else {
        panic!("expected DynCall variant");
    }
}

// A1-5: CoreExpr::DynCall CBOR round-trip preserves all fields.
#[test]
fn dyn_call_core_expr_cbor_round_trip() {
    let expr = CoreExpr::DynCall {
        interface: "Repository<User>".to_string(),
        method: "get".to_string(),
        args: vec![CoreExpr::Var("id".to_string())],
    };
    let bytes = stable_cbor_bytes(&expr).expect("encode");
    let decoded: CoreExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, expr, "DynCall must survive CBOR round-trip");
    if let CoreExpr::DynCall {
        interface,
        method,
        args,
    } = decoded
    {
        assert_eq!(interface, "Repository<User>");
        assert_eq!(method, "get");
        assert_eq!(args.len(), 1);
    } else {
        panic!("expected DynCall after round-trip");
    }
}

// A1-6 (TRIANGULATE): DynCall with multiple args round-trips.
#[test]
fn dyn_call_with_multiple_args_cbor_round_trip() {
    let expr = CoreExpr::DynCall {
        interface: "Serializable".to_string(),
        method: "serialize".to_string(),
        args: vec![
            CoreExpr::Var("value".to_string()),
            CoreExpr::Var("format".to_string()),
        ],
    };
    let bytes = stable_cbor_bytes(&expr).expect("encode");
    let decoded: CoreExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, expr);
    if let CoreExpr::DynCall { args, .. } = decoded {
        assert_eq!(args.len(), 2);
    } else {
        panic!("expected DynCall");
    }
}

// A1-7: DynCall backward compat — a CoreNode encoded before DynCall existed
// decodes successfully with expr = None.
// Spec scenario: "DynCall backward compat — absence from wire format"
#[test]
fn dyn_call_backward_compat_with_none_expr() {
    // Simulate a pre-DynCall CoreNode: expr field is None and is absent from CBOR.
    let node = CoreNode {
        source_ref: NodeRef(0),
        kind: CoreNodeKind::Function,
        name: "legacy_fn".to_string(),
        ty: None,
        expr: None,
    };
    let bytes = stable_cbor_bytes(&node).expect("encode legacy node");
    let decoded: CoreNode = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(
        decoded.expr, None,
        "legacy node decoded without DynCall must have expr=None"
    );
}

// ── Task F1 (RED): CoreType::BoundarySchema ───────────────────────────
// Tests written BEFORE the variant exists — compilation failure is the RED gate.

// F1-1: CoreType::BoundarySchema carries the schema name and round-trips through CBOR.
// Spec scenario: "BoundarySchema CBOR round-trip"

#[test]
fn new_core_expr_variants_cbor_round_trip() {
    let variants: Vec<CoreExpr> = vec![
        CoreExpr::ForEach {
            binding: "x".to_string(),
            collection: Box::new(CoreExpr::Var("xs".to_string())),
            body: Box::new(CoreExpr::Placeholder),
        },
        CoreExpr::Fold {
            init: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
            list: Box::new(CoreExpr::Var("items".to_string())),
            func: Box::new(CoreExpr::Var("sum".to_string())),
        },
        CoreExpr::Return {
            value: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
        },
        CoreExpr::MapNew {
            entries: vec![(
                CoreExpr::Literal(LiteralValue::Text("k".to_string())),
                CoreExpr::Literal(LiteralValue::Int(0)),
            )],
        },
        CoreExpr::SetNew {
            elements: vec![CoreExpr::Literal(LiteralValue::Int(42))],
        },
        CoreExpr::IndexGet {
            collection: Box::new(CoreExpr::Var("list".to_string())),
            index: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
        },
        CoreExpr::BoundaryCall {
            boundary: "payments".to_string(),
            func: "charge".to_string(),
            args: vec![CoreExpr::Var("id".to_string())],
        },
    ];
    for expr in &variants {
        let bytes = stable_cbor_bytes(expr).expect("encode");
        let decoded: CoreExpr = ciborium::from_reader(bytes.as_slice()).expect("decode");
        assert_eq!(
            &decoded, expr,
            "CoreExpr::{:?} must survive CBOR round-trip",
            expr
        );
    }
}
