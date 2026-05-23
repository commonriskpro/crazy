// ── ail-verify::report — extension field tests ────────────────────────────
//
// Strict TDD — RED phase.
// Tests for new additive fields on VerificationReport:
//   proof_obligations, degradation_events, artifact_hashes
//
// Spec: verification-pipeline/spec §1 (proof obligation pipeline with
// degradation tracking), §5 (codegen consistency).
// Design: additive serde-defaulted fields; older reports still deserialize.

use ail_verify::report::{ArtifactHash, DegradationEvent, VerificationReport, VerificationState};

// ── Scenario: new fields default to empty ─────────────────────────────────
// GIVEN a VerificationReport constructed via Default
// WHEN the three new fields are inspected
// THEN all three are empty slices
#[test]
fn new_report_fields_default_to_empty() {
    let report = VerificationReport::default();
    assert!(
        report.proof_obligations.is_empty(),
        "proof_obligations must default to empty"
    );
    assert!(
        report.degradation_events.is_empty(),
        "degradation_events must default to empty"
    );
    assert!(
        report.artifact_hashes.is_empty(),
        "artifact_hashes must default to empty"
    );
}

// ── Scenario: artifact_hashes round-trip through CBOR ────────────────────
// GIVEN a report with two ArtifactHash entries
// WHEN serialized to CBOR and deserialized
// THEN both hashes are preserved exactly
#[test]
fn artifact_hashes_round_trip_cbor() {
    use ciborium::from_reader;
    use ciborium::into_writer;

    let report = VerificationReport {
        artifact_hashes: vec![
            ArtifactHash {
                artifact: "canonical_change".into(),
                hash: "abc123".into(),
            },
            ArtifactHash {
                artifact: "core_ir".into(),
                hash: "def456".into(),
            },
        ],
        ..Default::default()
    };

    let mut buf = Vec::new();
    into_writer(&report, &mut buf).expect("CBOR serialization must succeed");
    let decoded: VerificationReport =
        from_reader(buf.as_slice()).expect("CBOR deserialization must succeed");

    assert_eq!(decoded.artifact_hashes.len(), 2);
    assert_eq!(decoded.artifact_hashes[0].artifact, "canonical_change");
    assert_eq!(decoded.artifact_hashes[0].hash, "abc123");
    assert_eq!(decoded.artifact_hashes[1].artifact, "core_ir");
    assert_eq!(decoded.artifact_hashes[1].hash, "def456");
}

// ── Scenario: degradation_event round-trip through CBOR ──────────────────
// GIVEN a report with one DegradationEvent
// WHEN serialized and deserialized
// THEN the event fields are preserved
#[test]
fn degradation_event_round_trip_cbor() {
    use ciborium::from_reader;
    use ciborium::into_writer;

    let report = VerificationReport {
        degradation_events: vec![DegradationEvent {
            obligation_id: "po_001".into(),
            source_stage: "resource".into(),
            from_state: VerificationState::Proven,
            to_state: VerificationState::Assumed,
            reason: "policy allows degradation at boundary".into(),
            repair_options: vec!["add_runtime_check".into()],
        }],
        ..Default::default()
    };

    let mut buf = Vec::new();
    into_writer(&report, &mut buf).expect("serialize");
    let decoded: VerificationReport = from_reader(buf.as_slice()).expect("deserialize");

    assert_eq!(decoded.degradation_events.len(), 1);
    let ev = &decoded.degradation_events[0];
    assert_eq!(ev.obligation_id, "po_001");
    assert_eq!(ev.source_stage, "resource");
    assert_eq!(ev.from_state, VerificationState::Proven);
    assert_eq!(ev.to_state, VerificationState::Assumed);
    assert_eq!(ev.repair_options, vec!["add_runtime_check".to_string()]);
}

// ── Scenario: old report without new fields deserializes cleanly ──────────
// GIVEN JSON produced before the new fields were added (no new keys present)
// WHEN deserialized into VerificationReport
// THEN new fields default to empty (backward compat)
#[test]
fn old_report_without_new_fields_deserializes_cleanly() {
    let old_json = r#"{
        "entries": [],
        "diagnostics": [],
        "schema_version": "verification/1.0",
        "summary_counts": {
            "verified_count": 0,
            "runtime_checked_count": 0,
            "assumed_count": 0,
            "unverified_count": 0,
            "unsafe_count": 0,
            "failed_count": 0
        }
    }"#;

    let report: VerificationReport =
        serde_json::from_str(old_json).expect("old report must deserialize");

    assert!(
        report.proof_obligations.is_empty(),
        "proof_obligations must default to empty for old reports"
    );
    assert!(
        report.degradation_events.is_empty(),
        "degradation_events must default to empty for old reports"
    );
    assert!(
        report.artifact_hashes.is_empty(),
        "artifact_hashes must default to empty for old reports"
    );
}

// ── TRIANGULATE: ArtifactHash with empty hash field ──────────────────────
// GIVEN an ArtifactHash with empty hash (edge case: not yet computed)
// WHEN stored in a report and round-tripped
// THEN preserved exactly — no coercion
#[test]
fn artifact_hash_with_empty_hash_round_trips() {
    use ciborium::from_reader;
    use ciborium::into_writer;

    let report = VerificationReport {
        artifact_hashes: vec![ArtifactHash {
            artifact: "wasm".into(),
            hash: String::new(), // not yet computed
        }],
        ..Default::default()
    };

    let mut buf = Vec::new();
    into_writer(&report, &mut buf).unwrap();
    let decoded: VerificationReport = from_reader(buf.as_slice()).unwrap();

    assert_eq!(decoded.artifact_hashes[0].hash, "");
}

// ── TRIANGULATE: proof_obligations field accepts ObligationLedgerEntry vec ─
// GIVEN a report with a proof_obligations vec (populated externally)
// WHEN summary() is called
// THEN summary is based on entries, not proof_obligations (they don't affect summary)
#[test]
fn proof_obligations_do_not_affect_summary() {
    use ail_verify::proof::{
        ClauseRole, ObligationAttempt, ObligationLedgerEntry, ObligationState, ProofObligation,
    };

    let entry = ObligationLedgerEntry {
        id: "po_001".into(),
        obligation: ProofObligation {
            predicate: "false".into(),
            role: ClauseRole::Requires,
            scope: "fn.checkout".into(),
        },
        state: ObligationState::Failed,
        source_stage: "contract".into(),
        attempts: vec![ObligationAttempt {
            stage: "simplify".into(),
            outcome: "failed".into(),
            evidence: None,
        }],
        degradation_reason: None,
        repair_options: vec![],
    };

    let report = VerificationReport {
        proof_obligations: vec![entry],
        ..Default::default()
    };

    // summary() is based on entries (none here), not proof_obligations
    assert_eq!(
        report.summary(),
        ail_verify::report::VerificationState::Proven,
        "summary depends only on entries"
    );
}
