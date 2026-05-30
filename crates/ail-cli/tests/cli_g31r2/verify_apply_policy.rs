use super::common::{ail, compute_sample_change_id, create_sample_change, parse_json_output};

// ── G31 R2: verify with --profile ─────────────────────────────────────────

/// SC-VER1: verify with --profile dev includes policy_report and approval_requirements.
#[test]
fn verify_profile_dev_has_policy_and_approval() {
    let change_id = compute_sample_change_id();
    let output = ail()
        .args(["verify", &change_id, "--profile", "dev", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["policy_report"].is_object(),
        "policy_report must be object; got: {v}"
    );
    assert!(
        v["data"]["approval_requirements"].is_object(),
        "approval_requirements must be object; got: {v}"
    );
    assert!(
        v["data"]["diagnostics"].is_array(),
        "diagnostics must be array; got: {v}"
    );
    assert!(
        v["data"]["proof_obligations"].is_array(),
        "proof_obligations must be array; got: {v}"
    );
    assert!(
        v["data"]["degradation_events"].is_array(),
        "degradation_events must be array; got: {v}"
    );
    assert!(
        v["data"]["solver_diagnostics"].is_array(),
        "solver_diagnostics must be array; got: {v}"
    );
    assert!(
        v["data"]["artifact_hashes"].is_array(),
        "artifact_hashes must be array; got: {v}"
    );
}

/// SC-VER2: verify with --profile prod has approval_requirements.required=true.
#[test]
fn verify_profile_prod_requires_approval() {
    let change_id = compute_sample_change_id();
    let output = ail()
        .args(["verify", &change_id, "--profile", "prod", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(
        v["data"]["approval_requirements"]["required"], true,
        "prod profile must require approval; got: {v}"
    );
    assert_eq!(
        v["data"]["policy_report"]["status"], "approval_required",
        "prod verify JSON must not report policy as ok while approval is required; got: {v}"
    );
    assert_eq!(
        v["data"]["policy_report"]["blocks_apply"], true,
        "prod verify JSON must make approval-required blocking state machine-readable; got: {v}"
    );
    assert_eq!(
        v["data"]["policy_report"]["policy_ok"], false,
        "prod verify JSON must not imply prod is OK before approval; got: {v}"
    );
}

#[test]
fn apply_prod_json_with_yes_marks_operator_confirmation_not_persisted_approval() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let change_id = create_sample_change(dir.path());
    // Verification gate: must verify with --profile prod before applying with --policy prod.
    // Verifying with a different profile (e.g. dev) would be rejected by the profile gate.
    ail()
        .args(["verify", &change_id, "--profile", "prod"])
        .current_dir(dir.path())
        .assert()
        .success();
    let output = ail()
        .args(["apply", &change_id, "--policy", "prod", "--yes", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(
        v["data"]["pre_apply_gate"]["approval_status"]["required"],
        true
    );
    assert_eq!(
        v["data"]["pre_apply_gate"]["approval_status"]["operator_confirmed"],
        true
    );
    assert_eq!(
        v["data"]["pre_apply_gate"]["approval_status"]["persisted_approval"],
        false
    );
    assert_eq!(
        v["data"]["pre_apply_gate"]["approval_status"]["satisfied_for_this_apply"],
        true
    );
    assert_eq!(
        v["data"]["pre_apply_gate"]["policy_status"]["status"],
        "operator_confirmed"
    );
    assert_eq!(
        v["data"]["pre_apply_gate"]["policy_status"]["approval_source"],
        "operator_confirmation"
    );
    assert_eq!(
        v["data"]["pre_apply_gate"]["policy_status"]["blocks_apply"],
        false
    );
}

// ── G31 R2: apply pre-apply gate ──────────────────────────────────────────

/// SC-APL1: apply --json includes pre_apply_gate with all required fields.
#[test]
fn apply_json_has_pre_apply_gate() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let change_id = create_sample_change(dir.path());
    // Verification gate: run verify before apply.
    ail()
        .args(["verify", &change_id])
        .current_dir(dir.path())
        .assert()
        .success();
    let output = ail()
        .args(["apply", &change_id, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    let gate = &v["data"]["pre_apply_gate"];
    assert!(gate.is_object(), "pre_apply_gate must be object; got: {v}");
    assert!(
        gate["canonical_change_hash"].is_string(),
        "gate.canonical_change_hash must be string; got: {gate}"
    );
    assert!(
        gate["structural_diff"].is_object(),
        "gate.structural_diff must be object; got: {gate}"
    );
    assert!(
        gate["verification_report_status"].is_string(),
        "gate.verification_report_status must be string; got: {gate}"
    );
    assert!(
        gate["policy_status"].is_object(),
        "gate.policy_status must be object; got: {gate}"
    );
    assert!(
        gate["approval_status"].is_object(),
        "gate.approval_status must be object; got: {gate}"
    );
    assert!(
        gate["target_snapshot"].is_string(),
        "gate.target_snapshot must be string; got: {gate}"
    );
}

// ── G31 R2: approve full model ────────────────────────────────────────────

/// SC-APR1: approve --json includes record_id, immutable flag, expires_on_canonical_diff_change.
#[test]
fn approve_json_has_full_immutable_record() {
    let change_id = "aa".repeat(32);
    let output = ail()
        .args([
            "approve",
            &change_id,
            "--for",
            "public_api_changed",
            "--role",
            "security",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["approved"], true);
    assert!(
        v["data"]["record_id"].is_string(),
        "record_id must be string; got: {v}"
    );
    assert_eq!(
        v["data"]["immutable"], true,
        "approval must be immutable; got: {v}"
    );
    assert_eq!(
        v["data"]["expires_on_canonical_diff_change"], true,
        "approval must expire on diff change; got: {v}"
    );
    assert_eq!(
        v["data"]["role"], "security",
        "role must be security; got: {v}"
    );
}

// ── G31 R2: reject full immutable model ──────────────────────────────────

/// SC-REJ1: reject --json includes record_id and immutable flag.
#[test]
fn reject_json_has_full_immutable_record() {
    let change_id = "aa".repeat(32);
    let output = ail()
        .args([
            "reject",
            &change_id,
            "--reason",
            "capability too broad",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["approved"], false);
    assert!(
        v["data"]["record_id"].is_string(),
        "record_id must be string; got: {v}"
    );
    assert_eq!(
        v["data"]["immutable"], true,
        "rejection must be immutable; got: {v}"
    );
}

// ── G31 R2: policy real behavior ─────────────────────────────────────────

/// SC-POL1: policy check --json includes violations array and rules_checked.
#[test]
fn policy_check_json_has_violations_and_rules_checked() {
    let change_id = "ab".repeat(32);
    let output = ail()
        .args(["policy", "check", &change_id, "--profile", "prod", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["violations"].is_array(),
        "violations must be array; got: {v}"
    );
    assert!(
        v["data"]["rules_checked"].is_array(),
        "rules_checked must be array; got: {v}"
    );
    assert_eq!(
        v["data"]["engine_status"], "blocked",
        "prod policy check over default graph must expose PolicyEngine blocked status; got: {v}"
    );
    assert!(
        v["data"]["engine_approval_required"].is_array(),
        "engine_approval_required must be a stable array field; got: {v}"
    );
}

/// SC-POL2: policy explain --json includes enforced_on field.
#[test]
fn policy_explain_json_has_enforced_on() {
    let output = ail()
        .args(["policy", "explain", "no_unverified_public_api", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["enforced_on"].is_array(),
        "enforced_on must be array; got: {v}"
    );
}

/// SC-POL3: policy set --json has record_type field.
#[test]
fn policy_set_json_has_record_type() {
    let output = ail()
        .args(["policy", "set", "max_new_capabilities=2", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["record_type"].is_string(),
        "record_type must be string; got: {v}"
    );
}
