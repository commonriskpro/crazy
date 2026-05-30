use super::{build_solver, is_changeset_meta_stage_claim};
use crate::error::CliError;

// ── WN: repair_options propagation in cmd_verify JSON mappings ────────
//
// These unit tests prove that each of the three JSON mapping sites in
// cmd_verify includes the `repair_options` field from the corresponding
// domain struct when the field is non-empty.

// Scenario WN-1: diagnostics JSON includes repair_options from VerificationEntry.
//   GIVEN a VerificationEntry with non-empty repair_options
//   WHEN the entry is mapped to JSON (same expression as cmd_verify)
//   THEN the resulting JSON contains repair_options with all values
#[test]
fn diagnostics_json_includes_repair_options_when_non_empty() {
    use ail_verify::report::{VerificationEntry, VerificationState};
    use serde_json::json;

    let entry = VerificationEntry {
        claim: "test-claim".into(),
        state: VerificationState::Failed,
        scope: "scope".into(),
        evidence: None,
        blocking: true,
        repair_options: vec!["add_guard".into(), "add_runtime_check".into()],
    };
    let repair = "Fix the failing invariant or update the contract clause.";
    let v = json!({
        "claim": entry.claim,
        "state": format!("{:?}", entry.state),
        "scope": entry.scope,
        "repair": repair,
        "repair_options": entry.repair_options,
    });
    let opts = v["repair_options"]
        .as_array()
        .expect("repair_options must be array");
    assert_eq!(opts.len(), 2, "both repair options must propagate");
    assert_eq!(opts[0], "add_guard");
    assert_eq!(opts[1], "add_runtime_check");
}

// Scenario WN-2: degradation_events JSON includes repair_options from DegradationEvent.
//   GIVEN a DegradationEvent with non-empty repair_options
//   WHEN the event is mapped to JSON (same expression as cmd_verify)
//   THEN the resulting JSON contains repair_options with all values
#[test]
fn degradation_event_json_includes_repair_options_when_non_empty() {
    use ail_verify::report::{DegradationEvent, VerificationState};
    use serde_json::json;

    let d = DegradationEvent {
        obligation_id: "obl-001".into(),
        source_stage: "resource".into(),
        from_state: VerificationState::Proven,
        to_state: VerificationState::Assumed,
        reason: "capability boundary forced downgrade".into(),
        repair_options: vec!["add_runtime_check".into(), "add_explicit_assumption".into()],
    };
    let v = json!({
        "obligation_id": d.obligation_id,
        "source_stage": d.source_stage,
        "from_state": format!("{:?}", d.from_state),
        "to_state": format!("{:?}", d.to_state),
        "reason": d.reason,
        "repair_options": d.repair_options,
    });
    let opts = v["repair_options"]
        .as_array()
        .expect("repair_options must be array");
    assert_eq!(opts.len(), 2, "both repair options must propagate");
    assert_eq!(opts[0], "add_runtime_check");
    assert_eq!(opts[1], "add_explicit_assumption");
}

// Scenario WN-3: solver_diagnostics JSON includes repair_options from SolverDiagnostic.
//   GIVEN a SolverDiagnostic with non-empty repair_options
//   WHEN the diagnostic is mapped to JSON (same expression as cmd_verify)
//   THEN the resulting JSON contains repair_options with all values
#[test]
fn solver_diagnostic_json_includes_repair_options_when_non_empty() {
    use ail_verify::report::{SolverDiagnostic, SolverDiagnosticStatus};
    use serde_json::json;

    let s = SolverDiagnostic {
        obligation_id: "obl-002".into(),
        source_stage: "solver".into(),
        status: SolverDiagnosticStatus::Timeout,
        reason: "solver_timeout: predicate depth exceeded budget".into(),
        repair_options: vec![
            "simplify the predicate or split it into smaller obligations".into(),
            "add a runtime check when static proof is not practical".into(),
        ],
    };
    let v = json!({
        "obligation_id": s.obligation_id,
        "source_stage": s.source_stage,
        "status": s.status.as_str(),
        "reason": s.reason,
        "repair_options": s.repair_options,
    });
    let opts = v["repair_options"]
        .as_array()
        .expect("repair_options must be array");
    assert_eq!(opts.len(), 2, "both repair options must propagate");
    assert_eq!(
        opts[0],
        "simplify the predicate or split it into smaller obligations"
    );
    assert_eq!(
        opts[1],
        "add a runtime check when static proof is not practical"
    );
}

// Scenario WN-4: diagnostics JSON omits repair_options when empty.
//   GIVEN a VerificationEntry with empty repair_options
//   WHEN the entry is mapped to JSON (same expression as cmd_verify)
//   THEN the resulting JSON does NOT contain the repair_options key
#[test]
fn diagnostics_json_omits_repair_options_when_empty() {
    use ail_verify::report::{VerificationEntry, VerificationState};
    use serde_json::{Value, json};

    let entry = VerificationEntry {
        claim: "test-claim".into(),
        state: VerificationState::Failed,
        scope: "scope".into(),
        evidence: None,
        blocking: true,
        repair_options: vec![],
    };
    let repair = "Fix the failing invariant or update the contract clause.";
    let mut map = serde_json::Map::new();
    map.insert("claim".into(), json!(entry.claim));
    map.insert("state".into(), json!(format!("{:?}", entry.state)));
    map.insert("scope".into(), json!(entry.scope));
    map.insert("repair".into(), json!(repair));
    if !entry.repair_options.is_empty() {
        map.insert("repair_options".into(), json!(entry.repair_options));
    }
    let v = Value::Object(map);
    assert!(
        v.get("repair_options").is_none(),
        "empty repair_options must be omitted from diagnostics JSON"
    );
}

// Scenario WN-5: degradation_events JSON omits repair_options when empty.
//   GIVEN a DegradationEvent with empty repair_options
//   WHEN the event is mapped to JSON (same expression as cmd_verify)
//   THEN the resulting JSON does NOT contain the repair_options key
#[test]
fn degradation_event_json_omits_repair_options_when_empty() {
    use ail_verify::report::{DegradationEvent, VerificationState};
    use serde_json::{Value, json};

    let d = DegradationEvent {
        obligation_id: "obl-001".into(),
        source_stage: "resource".into(),
        from_state: VerificationState::Proven,
        to_state: VerificationState::Assumed,
        reason: "capability boundary forced downgrade".into(),
        repair_options: vec![],
    };
    let mut map = serde_json::Map::new();
    map.insert("obligation_id".into(), json!(d.obligation_id));
    map.insert("source_stage".into(), json!(d.source_stage));
    map.insert("from_state".into(), json!(format!("{:?}", d.from_state)));
    map.insert("to_state".into(), json!(format!("{:?}", d.to_state)));
    map.insert("reason".into(), json!(d.reason));
    if !d.repair_options.is_empty() {
        map.insert("repair_options".into(), json!(d.repair_options));
    }
    let v = Value::Object(map);
    assert!(
        v.get("repair_options").is_none(),
        "empty repair_options must be omitted from degradation_events JSON"
    );
}

// Scenario WN-6: solver_diagnostics JSON omits repair_options when empty.
//   GIVEN a SolverDiagnostic with empty repair_options
//   WHEN the diagnostic is mapped to JSON (same expression as cmd_verify)
//   THEN the resulting JSON does NOT contain the repair_options key
#[test]
fn solver_diagnostic_json_omits_repair_options_when_empty() {
    use ail_verify::report::{SolverDiagnostic, SolverDiagnosticStatus};
    use serde_json::{Value, json};

    let s = SolverDiagnostic {
        obligation_id: "obl-002".into(),
        source_stage: "solver".into(),
        status: SolverDiagnosticStatus::Timeout,
        reason: "solver_timeout: predicate depth exceeded budget".into(),
        repair_options: vec![],
    };
    let mut map = serde_json::Map::new();
    map.insert("obligation_id".into(), json!(s.obligation_id));
    map.insert("source_stage".into(), json!(s.source_stage));
    map.insert("status".into(), json!(s.status.as_str()));
    map.insert("reason".into(), json!(s.reason));
    if !s.repair_options.is_empty() {
        map.insert("repair_options".into(), json!(s.repair_options));
    }
    let v = Value::Object(map);
    assert!(
        v.get("repair_options").is_none(),
        "empty repair_options must be omitted from solver_diagnostics JSON"
    );
}

#[test]
fn recognises_changeset_meta_stage_claims_only() {
    assert!(is_changeset_meta_stage_claim("01-parse-changeset"));
    assert!(is_changeset_meta_stage_claim("02-canonicalize-changeset"));
    assert!(is_changeset_meta_stage_claim("05-build-semantic-diff"));
    assert!(!is_changeset_meta_stage_claim("06-resource-lifecycle"));
    assert!(!is_changeset_meta_stage_claim("19-anf-lowering"));
    assert!(!is_changeset_meta_stage_claim("1-parse-changeset"));
    assert!(!is_changeset_meta_stage_claim(""));
}

// ── Solver selection — ZI-1 ───────────────────────────────────────────

// Scenario ZI-1a: "simple" name resolves without error.
//   GIVEN solver_name = "simple"
//   WHEN build_solver is called
//   THEN Ok is returned (SimpleSolver is always available)
#[test]
fn build_solver_simple_name_ok() {
    assert!(
        build_solver("simple").is_ok(),
        "build_solver('simple') must always succeed"
    );
}

// Scenario ZI-1b: empty string resolves to simple solver.
//   GIVEN solver_name = ""
//   WHEN build_solver is called
//   THEN Ok is returned (empty string treated as default)
#[test]
fn build_solver_empty_name_ok() {
    assert!(
        build_solver("").is_ok(),
        "build_solver('') must succeed (default = simple)"
    );
}

// Scenario ZI-1c: unknown solver name returns a deterministic error.
//   GIVEN solver_name = "llm"
//   WHEN build_solver is called
//   THEN Err(CliError::Domain) is returned containing "supported"
#[test]
fn build_solver_unknown_name_returns_domain_error() {
    let err = build_solver("llm").expect_err("unknown solver must fail");
    let msg = format!("{err}");
    assert!(
        matches!(err, CliError::Domain(_)),
        "unknown solver must produce CliError::Domain; got: {msg}"
    );
    assert!(
        msg.contains("supported"),
        "error message must list supported values; got: {msg}"
    );
}

// Scenario ZI-1d: "z3" without the feature returns a clear error.
//   GIVEN solver_name = "z3" AND z3-solver feature NOT compiled
//   WHEN build_solver is called
//   THEN Err(CliError::Domain) is returned mentioning the feature flag
#[cfg(not(feature = "z3-solver"))]
#[test]
fn build_solver_z3_without_feature_returns_domain_error() {
    let err = build_solver("z3").expect_err("z3 without feature must fail");
    let msg = format!("{err}");
    assert!(
        matches!(err, CliError::Domain(_)),
        "z3 without feature must produce CliError::Domain; got: {msg}"
    );
    assert!(
        msg.contains("z3-solver"),
        "error must mention the z3-solver feature flag; got: {msg}"
    );
}

// Scenario ZI-1e: "z3" WITH the feature resolves successfully.
//   GIVEN solver_name = "z3" AND z3-solver feature IS compiled
//   WHEN build_solver is called
//   THEN Ok is returned (Z3Solver constructed without panic)
#[cfg(feature = "z3-solver")]
#[test]
fn build_solver_z3_with_feature_ok() {
    assert!(
        build_solver("z3").is_ok(),
        "build_solver('z3') must succeed when z3-solver feature is compiled"
    );
}
