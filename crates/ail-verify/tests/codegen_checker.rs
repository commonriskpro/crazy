// ── ail-verify::codegen_checker tests ────────────────────────────────────
//
// Strict TDD — tests for codegen consistency verification.
// Spec: verification-pipeline/spec §5 (codegen consistency checker).

use ail_core::semantic_graph::{CapabilityReqs, GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::codegen_checker::{ArtifactEntry, CodegenChecker};
use ail_verify::report::VerificationState;

fn graph(nodes: Vec<GraphNode>) -> SemanticGraph {
    SemanticGraph {
        nodes,
        edges: vec![],
    }
}

fn capability_node(id: u32, name: &str) -> GraphNode {
    let mut node = GraphNode::new(NodeRef(id), NodeKind::Capability, name);
    node.capability_reqs = Some(CapabilityReqs { caps: vec![] });
    node
}

fn artifact(name: &str, expected: &str, actual: &str) -> ArtifactEntry {
    ArtifactEntry {
        name: name.to_string(),
        expected_hash: expected.to_string(),
        actual_hash: actual.to_string(),
    }
}

// ── Scenario: matching hashes → Proven ───────────────────────────────────
// GIVEN an artifact where expected_hash == actual_hash
// WHEN CodegenChecker::check_artifacts is called
// THEN state is Proven
#[test]
fn matching_hash_is_proven() {
    let artifacts = vec![artifact("canonical_change", "abc123", "abc123")];
    let report = CodegenChecker::check_artifacts(&artifacts);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].state, VerificationState::Proven);
    assert!(report.entries[0].evidence.is_none());
}

// ── Scenario: mismatched hashes → Failed ─────────────────────────────────
// GIVEN an artifact where expected_hash != actual_hash
// WHEN CodegenChecker::check_artifacts is called
// THEN state is Failed with evidence describing the mismatch
#[test]
fn mismatched_hash_is_failed() {
    let artifacts = vec![artifact("core_ir", "expected_hash", "different_hash")];
    let report = CodegenChecker::check_artifacts(&artifacts);
    assert_eq!(report.entries[0].state, VerificationState::Failed);
    let ev = report.entries[0].evidence.as_ref().unwrap();
    assert!(ev.contains("mismatch"));
    assert!(ev.contains("expected_hash"));
    assert!(ev.contains("different_hash"));
}

// ── Scenario: empty hash → Unverified ────────────────────────────────────
// GIVEN an artifact where expected_hash or actual_hash is empty
// WHEN CodegenChecker::check_artifacts is called
// THEN state is Unverified (not yet computed)
#[test]
fn empty_hash_is_unverified() {
    let artifacts = vec![
        artifact("wasm", "", "abc123"),   // empty expected
        artifact("anf_ir", "abc123", ""), // empty actual
    ];
    let report = CodegenChecker::check_artifacts(&artifacts);
    assert_eq!(report.entries.len(), 2);
    assert_eq!(report.entries[0].state, VerificationState::Unverified);
    assert_eq!(report.entries[1].state, VerificationState::Unverified);
}

// ── Scenario: empty artifacts → empty report ─────────────────────────────
#[test]
fn empty_artifacts_produces_empty_report() {
    let report = CodegenChecker::check_artifacts(&[]);
    assert!(report.entries.is_empty());
}

// ── Scenario: artifact hashes stored in report ────────────────────────────
// GIVEN artifacts passed to check_artifacts
// WHEN the report is returned
// THEN report.artifact_hashes contains the actual hashes
#[test]
fn artifact_hashes_stored_in_report() {
    let artifacts = vec![
        artifact("canonical_change", "hash_a", "hash_a"),
        artifact("core_ir", "hash_b", "hash_b"),
    ];
    let report = CodegenChecker::check_artifacts(&artifacts);
    assert_eq!(report.artifact_hashes.len(), 2);
    assert_eq!(report.artifact_hashes[0].artifact, "canonical_change");
    assert_eq!(report.artifact_hashes[0].hash, "hash_a");
}

// ── Scenario: scope format is "artifact:<name>" ──────────────────────────
#[test]
fn artifact_entry_scope_format() {
    let artifacts = vec![artifact("core_ir", "h1", "h1")];
    let report = CodegenChecker::check_artifacts(&artifacts);
    assert_eq!(report.entries[0].scope, "artifact:core_ir");
}

// ── Scenario: TRIANGULATE — mixed artifacts ────────────────────────────────
#[test]
fn mixed_artifacts_produce_correct_states() {
    let artifacts = vec![
        artifact("canonical_change", "abc", "abc"), // Proven
        artifact("core_ir", "abc", "xyz"),          // Failed
        artifact("wasm", "", ""),                   // Unverified
    ];
    let report = CodegenChecker::check_artifacts(&artifacts);
    assert_eq!(report.entries.len(), 3);
    assert_eq!(report.entries[0].state, VerificationState::Proven);
    assert_eq!(report.entries[1].state, VerificationState::Failed);
    assert_eq!(report.entries[2].state, VerificationState::Unverified);
}

// ── Scenario: summary reflects worst artifact state ───────────────────────
#[test]
fn summary_reflects_worst_artifact_state() {
    let artifacts = vec![
        artifact("a", "x", "x"), // Proven
        artifact("b", "x", "y"), // Failed
    ];
    let report = CodegenChecker::check_artifacts(&artifacts);
    assert_eq!(report.summary(), VerificationState::Failed);
}

// ── Manifest consistency: capability in graph AND manifest → Proven ────────
// GIVEN a Capability node in the graph AND the same name in manifest_caps
// WHEN CodegenChecker::check_manifest_consistency is called
// THEN state is Proven
#[test]
fn capability_in_graph_and_manifest_is_proven() {
    let g = graph(vec![capability_node(0, "database.write")]);
    let manifest_caps = vec!["database.write".to_string()];
    let report = CodegenChecker::check_manifest_consistency(&g, &manifest_caps);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].state, VerificationState::Proven);
}

// ── Manifest consistency: capability in graph but NOT in manifest → Failed ─
#[test]
fn capability_in_graph_not_in_manifest_is_failed() {
    let g = graph(vec![capability_node(0, "net.read")]);
    let manifest_caps = vec![]; // empty manifest
    let report = CodegenChecker::check_manifest_consistency(&g, &manifest_caps);
    assert_eq!(report.entries[0].state, VerificationState::Failed);
    assert!(
        report.entries[0]
            .evidence
            .as_ref()
            .unwrap()
            .contains("not present in capabilities manifest")
    );
}

// ── Manifest consistency: extra capability in manifest not in graph → Failed
#[test]
fn extra_manifest_capability_not_in_graph_is_failed() {
    let g = graph(vec![]); // empty graph
    let manifest_caps = vec!["fs.write".to_string()];
    let report = CodegenChecker::check_manifest_consistency(&g, &manifest_caps);
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].state, VerificationState::Failed);
}

// ── Manifest consistency: empty graph + empty manifest → empty report ──────
#[test]
fn empty_graph_empty_manifest_produces_empty_report() {
    let g = graph(vec![]);
    let report = CodegenChecker::check_manifest_consistency(&g, &[]);
    assert!(report.entries.is_empty());
}

// ── TRIANGULATE: non-capability nodes are skipped ─────────────────────────
#[test]
fn non_capability_nodes_skipped_in_manifest_check() {
    let g = graph(vec![
        GraphNode::new(NodeRef(0), NodeKind::Function, "fn_a"),
        capability_node(1, "database.read"),
    ]);
    let manifest_caps = vec!["database.read".to_string()];
    let report = CodegenChecker::check_manifest_consistency(&g, &manifest_caps);
    // Only 1 entry for the Capability node; Function node skipped
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].state, VerificationState::Proven);
}

// ── Production diagnostics: artifact mismatch is stable and redacted ─────
#[test]
fn artifact_mismatch_emits_redacted_stable_diagnostic() {
    use ail_verify::codegen_checker::VERIFY_CODEGEN_ARTIFACT_MISMATCH;

    let artifacts = vec![artifact(
        "wasm",
        "expected_secret_hash",
        "actual_secret_hash",
    )];
    let report = CodegenChecker::check_artifacts(&artifacts);

    let diagnostic = report
        .diagnostics
        .iter()
        .find(|d| d.code == VERIFY_CODEGEN_ARTIFACT_MISMATCH)
        .expect("artifact mismatch diagnostic");

    assert!(diagnostic.blocking);
    assert_eq!(diagnostic.expected.as_deref(), Some("hash:<redacted>"));
    assert_eq!(diagnostic.actual.as_deref(), Some("hash:<redacted>"));
    assert!(!format!("{diagnostic:?}").contains("expected_secret_hash"));
    assert!(!format!("{diagnostic:?}").contains("actual_secret_hash"));
}

// ── Production diagnostics: unsupported backend ──────────────────────────
#[test]
fn production_codegen_reports_unsupported_backend() {
    use ail_verify::codegen_checker::{
        CodegenVerificationContext, VERIFY_CODEGEN_UNSUPPORTED_BACKEND,
    };

    let report = CodegenChecker::check_production_codegen(CodegenVerificationContext {
        backend: "native-object",
        supported_backends: &["wasm"],
        artifacts: &[],
        proof_evidence_present: true,
        translation_evidence_present: true,
        output_deterministic: Some(true),
    });

    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].code,
        VERIFY_CODEGEN_UNSUPPORTED_BACKEND
    );
    assert!(report.diagnostics[0].blocking);
}

// ── Production diagnostics: proof and translation evidence ───────────────
#[test]
fn production_codegen_reports_missing_proof_and_translation_evidence() {
    use ail_verify::codegen_checker::{
        CodegenVerificationContext, VERIFY_CODEGEN_MISSING_PROOF_EVIDENCE,
        VERIFY_CODEGEN_MISSING_TRANSLATION_EVIDENCE,
    };

    let report = CodegenChecker::check_production_codegen(CodegenVerificationContext {
        backend: "wasm",
        supported_backends: &["wasm"],
        artifacts: &[],
        proof_evidence_present: false,
        translation_evidence_present: false,
        output_deterministic: Some(true),
    });

    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(
        codes,
        vec![
            VERIFY_CODEGEN_MISSING_PROOF_EVIDENCE,
            VERIFY_CODEGEN_MISSING_TRANSLATION_EVIDENCE,
        ]
    );
}

// ── Production diagnostics: nondeterministic codegen ─────────────────────
#[test]
fn production_codegen_reports_nondeterministic_output_when_represented() {
    use ail_verify::codegen_checker::{
        CodegenVerificationContext, VERIFY_CODEGEN_NONDETERMINISTIC_OUTPUT,
    };

    let report = CodegenChecker::check_production_codegen(CodegenVerificationContext {
        backend: "wasm",
        supported_backends: &["wasm"],
        artifacts: &[],
        proof_evidence_present: true,
        translation_evidence_present: true,
        output_deterministic: Some(false),
    });

    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(
        report.diagnostics[0].code,
        VERIFY_CODEGEN_NONDETERMINISTIC_OUTPUT
    );
}

// ── Production diagnostics: deterministic ordering and dedup ─────────────
#[test]
fn production_codegen_diagnostics_are_sorted_deduped_and_redacted() {
    use ail_verify::codegen_checker::{
        CodegenVerificationContext, VERIFY_CODEGEN_ARTIFACT_MISMATCH,
        VERIFY_CODEGEN_MISSING_PROOF_EVIDENCE, VERIFY_CODEGEN_MISSING_TRANSLATION_EVIDENCE,
        VERIFY_CODEGEN_NONDETERMINISTIC_OUTPUT, VERIFY_CODEGEN_UNSUPPORTED_BACKEND,
    };

    let artifacts = vec![
        artifact("wasm", "expected_secret_hash", "actual_secret_hash"),
        artifact("wasm", "expected_secret_hash", "actual_secret_hash"),
    ];
    let report = CodegenChecker::check_production_codegen(CodegenVerificationContext {
        backend: "native-object",
        supported_backends: &["wasm"],
        artifacts: &artifacts,
        proof_evidence_present: false,
        translation_evidence_present: false,
        output_deterministic: Some(false),
    });

    let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(
        codes,
        vec![
            VERIFY_CODEGEN_ARTIFACT_MISMATCH,
            VERIFY_CODEGEN_MISSING_PROOF_EVIDENCE,
            VERIFY_CODEGEN_MISSING_TRANSLATION_EVIDENCE,
            VERIFY_CODEGEN_NONDETERMINISTIC_OUTPUT,
            VERIFY_CODEGEN_UNSUPPORTED_BACKEND,
        ]
    );

    let rendered = format!("{:?}", report.diagnostics);
    assert!(!rendered.contains("expected_secret_hash"));
    assert!(!rendered.contains("actual_secret_hash"));
}
