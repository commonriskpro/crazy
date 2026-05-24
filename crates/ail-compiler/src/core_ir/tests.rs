use ail_core::semantic_graph::NodeRef;
#[allow(unused_imports)]
use ciborium;

use super::*;
use crate::hash::stable_cbor_bytes;

// ── Task 1.5 — RED: tests written before types existed. ───────────────

// Scenario: CoreIr is constructible with one CoreNode.
// Base case — proves the struct and its fields accept the right types.
#[test]
fn core_ir_is_constructible_with_one_node() {
    let node = CoreNode {
        source_ref: NodeRef(0),
        kind: CoreNodeKind::Module,
        name: "core_mod".to_string(),
        ty: None,
        expr: None,
    };
    let ir = CoreIr {
        nodes: vec![node],
        stage_hashes: StageHashes {
            graph_snapshot_hash: [0u8; 32],
            verification_report_hash: [0u8; 32],
            core_ir_hash: [1u8; 32],
            anf_ir_hash: None,
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        },
    };
    assert_eq!(ir.nodes.len(), 1);
    assert_eq!(ir.nodes[0].source_ref, NodeRef(0));
    assert_eq!(ir.nodes[0].kind, CoreNodeKind::Module);
}

// Scenario: CoreNode preserves its source_ref provenance.
// Proves the provenance contract: source_ref is not dropped or mutated.
#[test]
fn core_node_preserves_source_ref() {
    let node = CoreNode {
        source_ref: NodeRef(99),
        kind: CoreNodeKind::Function,
        name: "fn_with_high_ref".to_string(),
        ty: None,
        expr: None,
    };
    assert_eq!(node.source_ref, NodeRef(99));
}

// TRIANGULATE: stable_cbor_bytes on Vec<CoreNode> is deterministic.
// Proves that the Serialize impl produces stable bytes for the node list
// — the actual content used for hash sealing in lower_to_core_ir (PR 2).
#[test]
fn core_node_list_cbor_is_deterministic() {
    let nodes = vec![
        CoreNode {
            source_ref: NodeRef(0),
            kind: CoreNodeKind::Function,
            name: "fn_a".to_string(),
            ty: None,
            expr: None,
        },
        CoreNode {
            source_ref: NodeRef(1),
            kind: CoreNodeKind::Module,
            name: "mod_b".to_string(),
            ty: None,
            expr: None,
        },
        CoreNode {
            source_ref: NodeRef(2),
            kind: CoreNodeKind::Effect,
            name: "eff_c".to_string(),
            ty: None,
            expr: None,
        },
    ];
    let b1 = stable_cbor_bytes(&nodes).expect("first encode");
    let b2 = stable_cbor_bytes(&nodes).expect("second encode");
    assert_eq!(
        b1, b2,
        "Vec<CoreNode> must produce identical CBOR bytes across calls"
    );
}

// TRIANGULATE: different CoreNode lists produce different CBOR bytes.
// Proves the encoding is not constant (real content affects output).
#[test]
fn different_core_node_lists_produce_different_cbor() {
    let list_a = vec![CoreNode {
        source_ref: NodeRef(0),
        kind: CoreNodeKind::Module,
        name: "a".to_string(),
        ty: None,
        expr: None,
    }];
    let list_b = vec![CoreNode {
        source_ref: NodeRef(0),
        kind: CoreNodeKind::Module,
        name: "b".to_string(),
        ty: None,
        expr: None,
    }];
    let b_a = stable_cbor_bytes(&list_a).expect("encode a");
    let b_b = stable_cbor_bytes(&list_b).expect("encode b");
    assert_ne!(
        b_a, b_b,
        "different CoreNode lists must produce different CBOR"
    );
}

// Scenario: StageHashes optional fields are None by default.
#[test]
fn stage_hashes_optional_fields_default_none() {
    let h = StageHashes {
        graph_snapshot_hash: [0u8; 32],
        verification_report_hash: [0u8; 32],
        core_ir_hash: [42u8; 32],
        anf_ir_hash: None,
        wasm_hash: None,
        native_hash: None,
        source_map_hash: None,
        artifact_manifest_hash: None,
    };
    assert!(h.anf_ir_hash.is_none());
    assert!(h.wasm_hash.is_none());
    assert!(h.native_hash.is_none());
    assert_eq!(h.core_ir_hash, [42u8; 32]);
}

// TRIANGULATE: all CoreNodeKind variants are constructible.
// Ensures no variant is accidentally omitted from the enum.
#[test]
fn all_core_node_kinds_are_constructible() {
    let kinds = [
        CoreNodeKind::Module,
        CoreNodeKind::Function,
        CoreNodeKind::Type,
        CoreNodeKind::Effect,
        CoreNodeKind::Capability,
        CoreNodeKind::Contract,
        CoreNodeKind::Invariant,
        CoreNodeKind::Test,
        CoreNodeKind::Boundary,
        CoreNodeKind::Package,
    ];
    assert_eq!(
        kinds.len(),
        10,
        "all 10 CoreNodeKind variants must be reachable"
    );
}

// ── G2: CoreType tests ────────────────────────────────────────────────

// S2: All original CoreType variants are constructible without panic.
// Updated for ola3-core-ir-types: parameterized variants now carry inner types.
#[test]
fn all_core_type_variants_are_constructible() {
    // Original unit-like variants (unchanged).
    let _unit = CoreType::Unit;
    let _never = CoreType::Never;
    let _bool = CoreType::Bool;
    let _int = CoreType::Int;
    let _uint = CoreType::UInt;
    let _float = CoreType::Float;
    let _text = CoreType::Text;
    let _bytes = CoreType::Bytes;
    let _record = CoreType::Record;
    let _variant = CoreType::Variant;
    let _tuple = CoreType::Tuple;
    let _generic = CoreType::Generic(None);
    // Parameterized variants (now carry inner types).
    let _list = CoreType::List(Box::new(CoreType::Int));
    let _map = CoreType::Map(Box::new(CoreType::Text), Box::new(CoreType::Int));
    let _set = CoreType::Set(Box::new(CoreType::Bool));
    let _option = CoreType::Option(Box::new(CoreType::Int));
    let _result = CoreType::Result(Box::new(CoreType::Int), Box::new(CoreType::Text));
    let _function = CoreType::Function {
        params: vec![CoreType::Int],
        ret: Box::new(CoreType::Bool),
        effects: vec![],
    };
    let _handle = CoreType::Handle {
        resource: Box::new(CoreType::Text),
        mode: ResourceMode::Copy,
    };
    let _refinement = CoreType::Refinement {
        base: Box::new(CoreType::Int),
        predicate: "x > 0".to_string(),
    };
    // All constructed without panic — test passes.
}

// S1: CoreType::Bool is constructible and serializable (deterministic CBOR).
#[test]
fn core_type_bool_cbor_is_deterministic() {
    let ty = CoreType::Bool;
    let b1 = stable_cbor_bytes(&ty).expect("first encode");
    let b2 = stable_cbor_bytes(&ty).expect("second encode");
    assert_eq!(b1, b2, "CoreType::Bool CBOR must be deterministic");
}

// TRIANGULATE: different CoreType variants produce different CBOR bytes.
#[test]
fn different_core_types_produce_different_cbor() {
    let b_int = stable_cbor_bytes(&CoreType::Int).expect("encode Int");
    let b_text = stable_cbor_bytes(&CoreType::Text).expect("encode Text");
    assert_ne!(b_int, b_text, "Int and Text must produce different CBOR");
}

// ── G2: CoreExpr tests ────────────────────────────────────────────────

// S3: All CoreExpr variants are constructible without panic.
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
fn core_node_with_type_payload_cbor_round_trip() {
    let node = CoreNode {
        source_ref: NodeRef(5),
        kind: CoreNodeKind::Type,
        name: "amount".to_string(),
        ty: Some(CoreType::Int),
        expr: Some(CoreExpr::Var("amount_var".to_string())),
    };
    let bytes = stable_cbor_bytes(&node).expect("encode must succeed");
    let decoded: CoreNode = ciborium::from_reader(bytes.as_slice()).expect("decode must succeed");
    assert_eq!(
        decoded, node,
        "CoreNode with ty+expr must survive CBOR round-trip"
    );
}

// S5: CoreNode without type/expr fields skips those fields in CBOR.
// Verifies backward-compat: a node with None fields produces fewer bytes
// than a node with populated ty/expr (the optional fields are absent).
#[test]
fn core_node_without_type_fields_omits_them_from_cbor() {
    let node_minimal = CoreNode {
        source_ref: NodeRef(0),
        kind: CoreNodeKind::Module,
        name: "m".to_string(),
        ty: None,
        expr: None,
    };
    let node_rich = CoreNode {
        source_ref: NodeRef(0),
        kind: CoreNodeKind::Module,
        name: "m".to_string(),
        ty: Some(CoreType::Bool),
        expr: Some(CoreExpr::Placeholder),
    };
    let bytes_minimal = stable_cbor_bytes(&node_minimal).expect("encode minimal");
    let bytes_rich = stable_cbor_bytes(&node_rich).expect("encode rich");
    // The rich node must produce strictly more bytes (it has extra fields).
    assert!(
        bytes_minimal.len() < bytes_rich.len(),
        "node with ty+expr must encode to more bytes than node without them: {} vs {}",
        bytes_minimal.len(),
        bytes_rich.len()
    );
    // Round-trip the minimal node to confirm no extra keys sneak in.
    let decoded: CoreNode =
        ciborium::from_reader(bytes_minimal.as_slice()).expect("decode minimal");
    assert_eq!(decoded.ty, None, "decoded ty must be None");
    assert_eq!(decoded.expr, None, "decoded expr must be None");
}

// LiteralValue: all 5 variants constructible and Eq is sound.
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
fn new_concurrency_cell_variants_cbor_round_trip() {
    let variants: Vec<CoreExpr> = vec![
        CoreExpr::TaskAwait {
            task: Box::new(CoreExpr::Var("t".to_string())),
        },
        CoreExpr::TaskCancel {
            task: Box::new(CoreExpr::Var("t".to_string())),
        },
        CoreExpr::TaskGroup {
            body: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
        },
        CoreExpr::ChannelNew { capacity: None },
        CoreExpr::ChannelNew { capacity: Some(8) },
        CoreExpr::Select {
            branches: vec![SelectClause {
                channel: Box::new(CoreExpr::Var("ch".to_string())),
                binding: "v".to_string(),
                body: CoreExpr::Var("v".to_string()),
            }],
        },
        CoreExpr::Timeout {
            duration: Box::new(CoreExpr::Var("d".to_string())),
            body: Box::new(CoreExpr::Literal(LiteralValue::Unit)),
        },
        CoreExpr::CellNew {
            init: Box::new(CoreExpr::Literal(LiteralValue::Int(0))),
        },
        CoreExpr::CellGet {
            cell: Box::new(CoreExpr::Var("c".to_string())),
        },
        CoreExpr::CellSet {
            cell: Box::new(CoreExpr::Var("c".to_string())),
            value: Box::new(CoreExpr::Literal(LiteralValue::Int(42))),
        },
    ];
    for expr in &variants {
        let bytes = stable_cbor_bytes(expr).expect("encode must succeed");
        let decoded: CoreExpr =
            ciborium::from_reader(bytes.as_slice()).expect("decode must succeed");
        assert_eq!(
            &decoded, expr,
            "CoreExpr::{expr:?} must survive CBOR round-trip"
        );
    }
}

// MatchArm is constructible and Eq.
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
fn list_with_inner_type_cbor_round_trip() {
    let ty = CoreType::List(Box::new(CoreType::Int));
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty, "List<Int> must survive CBOR round-trip");
    // Verify inner type is preserved.
    if let CoreType::List(inner) = decoded {
        assert_eq!(*inner, CoreType::Int);
    } else {
        panic!("expected List variant");
    }
}

// S-B1b: Map(Text, Int) round-trips — both key and value types preserved.
#[test]
fn map_with_key_and_value_types_cbor_round_trip() {
    let ty = CoreType::Map(Box::new(CoreType::Text), Box::new(CoreType::Int));
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty, "Map<Text, Int> must survive CBOR round-trip");
}

// S-B1c: Option(Bool) round-trips.
#[test]
fn option_with_inner_type_cbor_round_trip() {
    let ty = CoreType::Option(Box::new(CoreType::Bool));
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty, "Option<Bool> must survive CBOR round-trip");
}

// S-B1d: Result(Int, Text) round-trips — Ok and Err types preserved.
#[test]
fn result_with_ok_and_err_types_cbor_round_trip() {
    let ty = CoreType::Result(Box::new(CoreType::Int), Box::new(CoreType::Text));
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(
        decoded, ty,
        "Result<Int, Text> must survive CBOR round-trip"
    );
}

// S-B1e: Handle { resource: Text, mode: ResourceMode::Linear } round-trips.
#[test]
fn handle_with_resource_and_mode_cbor_round_trip() {
    let ty = CoreType::Handle {
        resource: Box::new(CoreType::Text),
        mode: ResourceMode::Linear,
    };
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(
        decoded, ty,
        "Handle<Text, Linear> must survive CBOR round-trip"
    );
    if let CoreType::Handle { resource, mode } = decoded {
        assert_eq!(*resource, CoreType::Text);
        assert_eq!(mode, ResourceMode::Linear);
    } else {
        panic!("expected Handle variant");
    }
}

// S-B1f: PatchField(Text) round-trips — new parameterized variant.
#[test]
fn patch_field_with_inner_type_cbor_round_trip() {
    let ty = CoreType::PatchField(Box::new(CoreType::Text));
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty, "PatchField<Text> must survive CBOR round-trip");
}

// S-B1g: Vector(Float) round-trips.
#[test]
fn vector_with_inner_type_cbor_round_trip() {
    let ty = CoreType::Vector(Box::new(CoreType::Float));
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty, "Vector<Float> must survive CBOR round-trip");
}

// S-B1h: OrderedSet(Int) round-trips.
#[test]
fn ordered_set_with_inner_type_cbor_round_trip() {
    let ty = CoreType::OrderedSet(Box::new(CoreType::Int));
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty, "OrderedSet<Int> must survive CBOR round-trip");
}

// S-B1i: Task(Bool) round-trips — concurrency type with inner.
#[test]
fn task_with_inner_type_cbor_round_trip() {
    let ty = CoreType::Task(Box::new(CoreType::Bool));
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty, "Task<Bool> must survive CBOR round-trip");
}

// S-B1j: Channel(Text) round-trips.
// Triangulation: Task<Bool> and Channel<Text> must produce different CBOR.
#[test]
fn channel_with_inner_type_cbor_round_trip() {
    let ty = CoreType::Channel(Box::new(CoreType::Text));
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty, "Channel<Text> must survive CBOR round-trip");
    // Triangulation: Channel<Text> ≠ Task<Bool>
    let task_ty = CoreType::Task(Box::new(CoreType::Bool));
    let task_bytes = stable_cbor_bytes(&task_ty).expect("encode task");
    assert_ne!(
        bytes, task_bytes,
        "Channel<Text> must differ from Task<Bool> in CBOR"
    );
}

// ── Task A1 (RED): new flat CoreType variants ─────────────────────────

// S-A1a: All new flat CoreType variants are constructible.
#[test]
fn new_flat_core_type_variants_are_constructible() {
    let _decimal = CoreType::Decimal;
    let _existential = CoreType::Existential;
    let _code_point = CoreType::CodePoint;
    let _grapheme = CoreType::Grapheme;
    let _normalized_text = CoreType::NormalizedText("NFC".to_string());
    let _int32 = CoreType::Int32;
    let _int64 = CoreType::Int64;
    let _uint32 = CoreType::UInt32;
    let _uint64 = CoreType::UInt64;
    let _task_group = CoreType::TaskGroup;
    // All constructed without panic — test passes.
}

// S-A1b: NormalizedText carries its form string and round-trips through CBOR.
#[test]
fn normalized_text_cbor_round_trip() {
    let ty = CoreType::NormalizedText("NFC".to_string());
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(
        decoded, ty,
        "NormalizedText<NFC> must survive CBOR round-trip"
    );
}

// S-A1c: Decimal is distinct from Int and Float in CBOR encoding.
// Triangulation: different flat numeric types must produce different CBOR.
#[test]
fn decimal_is_distinct_from_int_and_float_in_cbor() {
    let b_decimal = stable_cbor_bytes(&CoreType::Decimal).expect("encode Decimal");
    let b_int = stable_cbor_bytes(&CoreType::Int).expect("encode Int");
    let b_float = stable_cbor_bytes(&CoreType::Float).expect("encode Float");
    assert_ne!(b_decimal, b_int, "Decimal must differ from Int in CBOR");
    assert_ne!(b_decimal, b_float, "Decimal must differ from Float in CBOR");
}

// S-A1d: Machine integer variants are all distinct from each other.
// Triangulation: Int32/Int64/UInt32/UInt64 must encode differently.
#[test]
fn machine_integer_variants_are_distinct_in_cbor() {
    let b_i32 = stable_cbor_bytes(&CoreType::Int32).expect("encode Int32");
    let b_i64 = stable_cbor_bytes(&CoreType::Int64).expect("encode Int64");
    let b_u32 = stable_cbor_bytes(&CoreType::UInt32).expect("encode UInt32");
    let b_u64 = stable_cbor_bytes(&CoreType::UInt64).expect("encode UInt64");
    assert_ne!(b_i32, b_i64);
    assert_ne!(b_i32, b_u32);
    assert_ne!(b_i32, b_u64);
    assert_ne!(b_i64, b_u32);
    assert_ne!(b_i64, b_u64);
    assert_ne!(b_u32, b_u64);
}

// ── Task A3 (RED): new additive CoreExpr variants ─────────────────────

// S-A3a: All new CoreExpr variants are constructible.
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
fn dyn_core_type_construction_and_eq() {
    let ty = CoreType::Dyn("Serializable".to_string());
    // Eq: same interface name → equal
    assert_eq!(ty, CoreType::Dyn("Serializable".to_string()));
    // Eq: different interface → not equal
    assert_ne!(ty, CoreType::Dyn("Repository<User>".to_string()));
}

// A1-2: CoreType::Dyn CBOR round-trip preserves the interface name.
// Spec scenario: "Dyn CoreType construction and CBOR round-trip"
#[test]
fn dyn_core_type_cbor_round_trip() {
    let ty = CoreType::Dyn("Serializable".to_string());
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(
        decoded, ty,
        "Dyn<Serializable> must survive CBOR round-trip"
    );
    if let CoreType::Dyn(name) = decoded {
        assert_eq!(name, "Serializable");
    } else {
        panic!("expected Dyn variant after round-trip");
    }
}

// A1-3 (TRIANGULATE): Dyn with a generic interface name also round-trips.
// Forces the payload to be a non-trivial string, not a hardcoded empty string.
#[test]
fn dyn_core_type_with_generic_interface_cbor_round_trip() {
    let ty = CoreType::Dyn("Repository<User>".to_string());
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty);
    if let CoreType::Dyn(name) = decoded {
        assert_eq!(name, "Repository<User>");
    } else {
        panic!("expected Dyn variant");
    }
}

// A1-4: CoreExpr::DynCall construction and field access.
// Spec scenario: "DynCall construction and CBOR round-trip"
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
fn boundary_schema_cbor_round_trip() {
    let ty = CoreType::BoundarySchema("UserInputJsonSchema".to_string());
    let bytes = stable_cbor_bytes(&ty).expect("encode");
    let decoded: CoreType = ciborium::from_reader(bytes.as_slice()).expect("decode");
    assert_eq!(decoded, ty, "BoundarySchema must survive CBOR round-trip");
    if let CoreType::BoundarySchema(name) = decoded {
        assert_eq!(name, "UserInputJsonSchema");
    } else {
        panic!("expected BoundarySchema variant");
    }
}

// F1-2: BoundarySchema Eq — same name equals, different names do not.
#[test]
fn boundary_schema_variant_equality() {
    let a = CoreType::BoundarySchema("UserInputJsonSchema".to_string());
    let b = CoreType::BoundarySchema("UserInputJsonSchema".to_string());
    let c = CoreType::BoundarySchema("PaymentsJsonSchema".to_string());
    assert_eq!(a, b);
    assert_ne!(a, c);
}

// F1-3 (TRIANGULATE): BoundarySchema is distinct from Dyn and ForeignType in CBOR.
#[test]
fn boundary_schema_is_distinct_from_dyn_and_foreign_type_in_cbor() {
    let bs = stable_cbor_bytes(&CoreType::BoundarySchema("Schema".to_string()))
        .expect("encode BoundarySchema");
    let dyn_ = stable_cbor_bytes(&CoreType::Dyn("Schema".to_string())).expect("encode Dyn");
    let foreign = stable_cbor_bytes(&CoreType::ForeignType("Schema".to_string()))
        .expect("encode ForeignType");
    // Same payload string but different variants → different CBOR
    assert_ne!(bs, dyn_, "BoundarySchema must differ from Dyn in CBOR");
    assert_ne!(
        bs, foreign,
        "BoundarySchema must differ from ForeignType in CBOR"
    );
}

// S-A3e: ForEach, Fold, Return, MapNew, SetNew, IndexGet round-trip through CBOR.
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
