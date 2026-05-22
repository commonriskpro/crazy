use ail_change::apply::SnapshotBridge;
use ail_change::{
    apply::apply,
    canonical::canonicalize_parsed,
    model::{ChangeSetOutcome, SnapshotId},
    parser::parse_changeset,
};
use ail_compiler::{emit_wasm, lower_to_anf, lower_to_core_ir};
use ail_core::semantic_graph::SemanticGraph;
use ail_runtime::{
    CapabilityManifest, ResourceLimits, RuntimeHost, RuntimeProfile, RuntimeValue, blake3_hex_of,
};
use ail_verify::report::VerificationReport;

struct FixedSnapshot;

impl SnapshotBridge for FixedSnapshot {
    fn current_snapshot_id(&self) -> SnapshotId {
        SnapshotId(0)
    }
}

#[test]
fn text_to_runtime_result_returns_42() {
    let source = "change e2e base=0\nauthor tester\ndescription e2e\nop create_function id=fn.answer return=Int value=42\nend\n";
    let parsed = parse_changeset(source).expect("source must parse");
    let canonical = canonicalize_parsed(parsed);
    let mut graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };

    let outcome = apply(canonical, &mut graph, &FixedSnapshot);
    assert_eq!(outcome, ChangeSetOutcome::Applied);

    let report = VerificationReport {
        entries: vec![],
        ..Default::default()
    };
    let core = lower_to_core_ir(&graph, &report).expect("core lowering must succeed");
    let anf = lower_to_anf(&core).expect("anf lowering must succeed");
    let artifact = emit_wasm(&anf).expect("wasm emission must succeed");

    let manifest = CapabilityManifest {
        module: "e2e".to_string(),
        requires: vec![],
    };
    let profile = RuntimeProfile::new(
        "test".to_string(),
        blake3_hex_of(&artifact.wasm),
        String::new(),
        manifest.blake3_hex().expect("manifest hash must compute"),
        vec![],
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
        },
    );
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&artifact.wasm, &manifest, &profile)
        .expect("runtime instantiation must succeed");

    let value = instance.invoke("answer", &[]).expect("invoke must succeed");

    assert_eq!(value, RuntimeValue::I64(42));
}

#[test]
fn more_ops_parse_canonicalize_apply_compile() {
    let source = "\
change more_ops base=0
author tester
description more ops e2e
op create_module id=module.checkout
op create_capability id=cap.payment.charge
op create_function id=fn.checkout return=Int value=42
op add_param target=fn.checkout name=cart_id type=CartId
op set_return target=fn.checkout type=Int
op set_body target=fn.checkout body=@expr.checkout
op add_effect target=fn.checkout effect=payment.charge
op add_contract target=fn.checkout kind=ensures rule=order_created
op connect source=fn.checkout_v2 relation=uses target=cap.payment.charge
op grant target=module.checkout capability=payment.charge
op rename target=fn.checkout name=fn.checkout_v2
op move target=fn.checkout_v2 to=module.checkout
op deprecate target=fn.checkout_v2 replacement=fn.checkout_v3
op annotate target=fn.checkout_v2 key=rationale value=idempotent
end
";
    let parsed = parse_changeset(source).expect("source must parse");
    let canonical = canonicalize_parsed(parsed);
    let mut graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };

    let outcome = apply(canonical, &mut graph, &FixedSnapshot);
    assert_eq!(outcome, ChangeSetOutcome::Applied);

    let report = VerificationReport {
        entries: vec![],
        ..Default::default()
    };
    let core = lower_to_core_ir(&graph, &report).expect("core lowering must succeed");
    let anf = lower_to_anf(&core).expect("anf lowering must succeed");
    emit_wasm(&anf).expect("wasm emission must succeed");
}

#[test]
fn remove_and_disconnect_ops_parse_canonicalize_apply_compile() {
    let source = "\
change remove_ops base=0
author tester
description remove ops e2e
op create_capability id=cap.payment.charge
op create_function id=fn.checkout return=Int value=42
op add_effect target=fn.checkout effect=payment.charge
op add_contract target=fn.checkout kind=ensures rule=order_created
op connect source=fn.checkout relation=uses target=cap.payment.charge
op remove_effect target=fn.checkout effect=payment.charge
op remove_contract target=fn.checkout rule=order_created
op disconnect source=fn.checkout relation=uses target=cap.payment.charge
op delete target=cap.payment.charge
end
";
    let parsed = parse_changeset(source).expect("source must parse");
    let canonical = canonicalize_parsed(parsed);
    let mut graph = SemanticGraph {
        nodes: vec![],
        edges: vec![],
    };

    let outcome = apply(canonical, &mut graph, &FixedSnapshot);
    assert_eq!(outcome, ChangeSetOutcome::Applied);

    let report = VerificationReport {
        entries: vec![],
        ..Default::default()
    };
    let core = lower_to_core_ir(&graph, &report).expect("core lowering must succeed");
    let anf = lower_to_anf(&core).expect("anf lowering must succeed");
    emit_wasm(&anf).expect("wasm emission must succeed");
}
