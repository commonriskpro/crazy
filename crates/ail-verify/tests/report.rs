// ── ail-verify::report tests ──────────────────────────────────────────────
//
// Strict TDD — RED phase.  These tests are written BEFORE src/report.rs
// exists; they define the acceptance criteria from the spec.
//
// Spec: verification-report-states domain
//   - Six-state enum: Proven, RuntimeChecked, Assumed, Unverified, Unsafe, Failed
//   - Summary priority: Failed > Unsafe > Unverified > Assumed > RuntimeChecked > Proven
//   - Entry structure: claim, state, scope, evidence: Option<String>
//   - evidence: None must NOT be coerced to empty string on serde

use ail_verify::report::{VerificationEntry, VerificationReport, VerificationState};

// ── Scenario: All six variants representable ──────────────────────────────

#[test]
fn all_six_states_are_representable() {
    let states = [
        VerificationState::Proven,
        VerificationState::RuntimeChecked,
        VerificationState::Assumed,
        VerificationState::Unverified,
        VerificationState::Unsafe,
        VerificationState::Failed,
    ];
    assert_eq!(states.len(), 6, "must have exactly six distinct states");
    // Verify each state is distinct (PartialEq must be derived)
    for (i, a) in states.iter().enumerate() {
        for (j, b) in states.iter().enumerate() {
            if i == j {
                assert_eq!(a, b, "state[{i}] must equal itself");
            } else {
                assert_ne!(a, b, "state[{i}] and state[{j}] must differ");
            }
        }
    }
}

// ── Scenario: Mixed states resolve to highest priority (Failed) ────────────

#[test]
fn mixed_states_summary_is_failed() {
    let report = VerificationReport {
        entries: vec![
            VerificationEntry {
                claim: "type".into(),
                state: VerificationState::Proven,
                scope: "node_a".into(),
                evidence: None,
                blocking: false,
                repair_options: vec![],
            },
            VerificationEntry {
                claim: "effect".into(),
                state: VerificationState::Assumed,
                scope: "node_a".into(),
                evidence: None,
                blocking: false,
                repair_options: vec![],
            },
            VerificationEntry {
                claim: "cap".into(),
                state: VerificationState::Failed,
                scope: "node_a".into(),
                evidence: Some("failed invariant".into()),
                blocking: true,
                repair_options: vec![],
            },
        ],
        ..Default::default()
    };
    assert_eq!(report.summary(), VerificationState::Failed);
}

// ── Scenario: Unsafe beats Unverified/Assumed/RuntimeChecked/Proven ────────

#[test]
fn unsafe_beats_unverified_and_assumed() {
    let report = VerificationReport {
        entries: vec![
            VerificationEntry {
                claim: "type".into(),
                state: VerificationState::Unverified,
                scope: "n1".into(),
                evidence: None,
                blocking: false,
                repair_options: vec![],
            },
            VerificationEntry {
                claim: "effect".into(),
                state: VerificationState::Assumed,
                scope: "n1".into(),
                evidence: None,
                blocking: false,
                repair_options: vec![],
            },
            VerificationEntry {
                claim: "boundary".into(),
                state: VerificationState::Unsafe,
                scope: "n1".into(),
                evidence: None,
                blocking: true,
                repair_options: vec![],
            },
        ],
        ..Default::default()
    };
    assert_eq!(report.summary(), VerificationState::Unsafe);
}

// ── Scenario: All-proven report yields Proven ─────────────────────────────

#[test]
fn all_proven_summary_is_proven() {
    let report = VerificationReport {
        entries: vec![
            VerificationEntry {
                claim: "type_a".into(),
                state: VerificationState::Proven,
                scope: "node_a".into(),
                evidence: None,
                blocking: false,
                repair_options: vec![],
            },
            VerificationEntry {
                claim: "type_b".into(),
                state: VerificationState::Proven,
                scope: "node_b".into(),
                evidence: None,
                blocking: false,
                repair_options: vec![],
            },
        ],
        ..Default::default()
    };
    assert_eq!(report.summary(), VerificationState::Proven);
}

// ── Scenario: Empty report yields Proven (vacuous truth) ──────────────────

#[test]
fn empty_report_summary_is_proven() {
    let report = VerificationReport {
        entries: vec![],
        ..Default::default()
    };
    assert_eq!(report.summary(), VerificationState::Proven);
}

// ── Scenario: RuntimeChecked priority between Proven and Assumed ───────────

#[test]
fn runtime_checked_beats_proven_but_not_assumed() {
    // RuntimeChecked > Proven
    let report_rt = VerificationReport {
        entries: vec![
            VerificationEntry {
                claim: "a".into(),
                state: VerificationState::Proven,
                scope: "n".into(),
                evidence: None,
                blocking: false,
                repair_options: vec![],
            },
            VerificationEntry {
                claim: "b".into(),
                state: VerificationState::RuntimeChecked,
                scope: "n".into(),
                evidence: None,
                blocking: false,
                repair_options: vec![],
            },
        ],
        ..Default::default()
    };
    assert_eq!(report_rt.summary(), VerificationState::RuntimeChecked);

    // Assumed > RuntimeChecked
    let report_assumed = VerificationReport {
        entries: vec![
            VerificationEntry {
                claim: "a".into(),
                state: VerificationState::RuntimeChecked,
                scope: "n".into(),
                evidence: None,
                blocking: false,
                repair_options: vec![],
            },
            VerificationEntry {
                claim: "b".into(),
                state: VerificationState::Assumed,
                scope: "n".into(),
                evidence: None,
                blocking: false,
                repair_options: vec![],
            },
        ],
        ..Default::default()
    };
    assert_eq!(report_assumed.summary(), VerificationState::Assumed);
}

// ── Scenario: Evidence None absent on serde (not coerced to empty string) ──

#[test]
fn evidence_none_is_absent_in_cbor_not_empty_string() {
    use ciborium::from_reader;
    use ciborium::into_writer;

    let entry = VerificationEntry {
        claim: "type_check".into(),
        state: VerificationState::Proven,
        scope: "my_node".into(),
        evidence: None,
        blocking: false,
        repair_options: vec![],
    };

    // Round-trip through CBOR
    let mut buf = Vec::new();
    into_writer(&entry, &mut buf).expect("CBOR serialization must succeed");
    let decoded: VerificationEntry =
        from_reader(buf.as_slice()).expect("CBOR deserialization must succeed");

    // evidence must still be None — not coerced to "" or Some("")
    assert_eq!(
        decoded.evidence, None,
        "evidence: None must survive CBOR round-trip as None"
    );
    assert_eq!(decoded.claim, "type_check");
    assert_eq!(decoded.scope, "my_node");
}

// ── Scenario: Entry with evidence Some(String) round-trips ────────────────

#[test]
fn evidence_some_round_trips_via_cbor() {
    use ciborium::from_reader;
    use ciborium::into_writer;

    let entry = VerificationEntry {
        claim: "invariant_check".into(),
        state: VerificationState::Failed,
        scope: "contract_node".into(),
        evidence: Some("contradiction found in nominal type".into()),
        blocking: true,
        repair_options: vec![],
    };

    let mut buf = Vec::new();
    into_writer(&entry, &mut buf).expect("serialize");
    let decoded: VerificationEntry = from_reader(buf.as_slice()).expect("deserialize");

    assert_eq!(
        decoded.evidence,
        Some("contradiction found in nominal type".into())
    );
    assert_eq!(decoded.state, VerificationState::Failed);
}

// ── Scenario: CI canonicalization dedupes entries without losing chronology ─

#[test]
fn canonicalize_for_ci_dedupes_entries_preserving_first_seen_order() {
    let duplicate = VerificationEntry {
        claim: "stage-a".into(),
        state: VerificationState::Failed,
        scope: "node-a".into(),
        evidence: Some("same failure".into()),
        blocking: true,
        repair_options: vec![],
    };
    let later = VerificationEntry {
        claim: "stage-b".into(),
        state: VerificationState::Proven,
        scope: "node-b".into(),
        evidence: None,
        blocking: false,
        repair_options: vec![],
    };
    let mut report = VerificationReport {
        entries: vec![duplicate.clone(), later.clone(), duplicate],
        ..Default::default()
    };

    report.canonicalize_for_ci();

    assert_eq!(
        report.entries,
        vec![
            VerificationEntry {
                claim: "stage-a".into(),
                state: VerificationState::Failed,
                scope: "node-a".into(),
                evidence: Some("same failure".into()),
                blocking: true,
                repair_options: vec![],
            },
            later,
        ]
    );
    assert_eq!(report.summary_counts.failed_count, 1);
    assert_eq!(report.summary_counts.verified_count, 1);
}

#[test]
fn canonicalize_for_ci_sorts_and_dedupes_diagnostics() {
    use ail_core::semantic_graph::NodeRef;
    use ail_verify::diagnostic::{Diagnostic, E_EFFECT_UNUSED, E_TYPE_MISMATCH};

    let error = Diagnostic::error(E_TYPE_MISMATCH, NodeRef(1)).with_actual("String");
    let warning = Diagnostic::warning(E_EFFECT_UNUSED, NodeRef(2));
    let mut report = VerificationReport {
        diagnostics: vec![warning.clone(), error.clone(), error.clone()],
        ..Default::default()
    };

    report.canonicalize_for_ci();

    assert_eq!(report.diagnostics, vec![error, warning]);
}
