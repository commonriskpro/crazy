use ail_change::{apply::apply, canonical::canonicalize_parsed, model::{ChangeSetOutcome, SnapshotId}, parser::parse_changeset};
use ail_change::apply::SnapshotBridge;
use ail_compiler::{emit_wasm, lower_to_anf, lower_to_core_ir};
use ail_core::semantic_graph::SemanticGraph;
use ail_runtime::{CapabilityManifest, ResourceLimits, RuntimeHost, RuntimeProfile, RuntimeValue, blake3_hex_of};
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
    let mut graph = SemanticGraph { nodes: vec![], edges: vec![] };

    let outcome = apply(canonical, &mut graph, &FixedSnapshot);
    assert_eq!(outcome, ChangeSetOutcome::Applied);

    let report = VerificationReport { entries: vec![], ..Default::default() };
    let core = lower_to_core_ir(&graph, &report).expect("core lowering must succeed");
    let anf = lower_to_anf(&core).expect("anf lowering must succeed");
    let artifact = emit_wasm(&anf).expect("wasm emission must succeed");

    let manifest = CapabilityManifest { module: "e2e".to_string(), requires: vec![] };
    let profile = RuntimeProfile::new(
        "test".to_string(),
        blake3_hex_of(&artifact.wasm),
        String::new(),
        manifest.blake3_hex().expect("manifest hash must compute"),
        vec![],
        ResourceLimits { max_memory_bytes: None, max_fuel: None },
    );
    let mut host = RuntimeHost::new();
    let mut instance = host
        .validate_and_instantiate(&artifact.wasm, &manifest, &profile)
        .expect("runtime instantiation must succeed");

    let value = instance.invoke("answer", &[]).expect("invoke must succeed");

    assert_eq!(value, RuntimeValue::I64(42));
}
