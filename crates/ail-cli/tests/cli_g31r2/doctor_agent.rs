use super::common::{ail, parse_json_output, sample_acl_path};

// ── G31 R2: doctor real checks ────────────────────────────────────────────

/// SC-DOC1: doctor --json has overall field and all 7 required check names.
#[test]
fn doctor_json_has_overall_and_all_check_names() {
    let output = ail()
        .args(["doctor", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["overall"].is_string(),
        "overall must be string; got: {v}"
    );

    let checks = v["data"]["checks"]
        .as_array()
        .expect("checks must be array");
    let check_names: Vec<&str> = checks.iter().filter_map(|c| c["name"].as_str()).collect();
    let expected_names = vec![
        "graph_integrity",
        "index_freshness",
        "schema_compatibility",
        "artifact_hash_consistency",
        "runtime_profile_validity",
        "package_advisories",
        "assumption_expirations",
    ];

    assert_eq!(
        check_names, expected_names,
        "doctor checks must preserve deterministic contract order"
    );

    for check in checks {
        assert!(
            check["code"].is_string(),
            "check code must be present: {check}"
        );
        assert!(
            check["category"].is_string(),
            "check category must be present: {check}"
        );
        assert!(
            check["redacted"].is_boolean(),
            "check redacted flag must be present: {check}"
        );
    }

    assert_eq!(checks[0]["code"], "AIL_DOCTOR_GRAPH_INTEGRITY");
    assert_eq!(checks[0]["category"], "graph");
    assert_eq!(checks[0]["redacted"], false);
}

// ── T7d: LLM agent loop E2E test ─────────────────────────────────────────

/// Spec scenario LL-1a: full LLM agent loop succeeds.
///
/// Exercises the 6-step protocol end-to-end using a file-backed store so that
/// cmd_change persists the CanonicalChangeSet and cmd_verify can load it.
///
/// Steps (matching tooling.md LLM protocol):
///  1. `ail context fn.checkout --json`  → schema_version = "1"
///  2. `ail impact type.CartItem.price --json` → schema_version = "1"
///  3. `ail change --file sample.acl --json` → change_id extracted
///  4. `ail verify <change_id> --profile dev --json` → policy_report present
///  5. `ail diff --semantic change.add_checkout --json` → exits 0
///  6. `ail apply <change_id> --json` → new_snapshot_id present
///
/// All steps assert schema_version == "1".
#[test]
fn llm_agent_loop_e2e_with_schema_version() {
    use assert_fs::TempDir;

    let dir = TempDir::new().expect("temp dir");

    // Initialize the file store so changeset payloads are persisted across calls.
    ail().arg("init").current_dir(dir.path()).assert().success();

    let path = sample_acl_path();

    // ── Step 1: context (no target — lists snapshots after init) ─────────
    let ctx_output = ail()
        .args(["context", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let ctx_json = parse_json_output(&ctx_output);
    assert_eq!(ctx_json["status"], "ok", "step 1 (context) must succeed");
    assert_eq!(
        ctx_json["data"]["schema_version"], "1",
        "step 1 (context): schema_version must be \"1\"; got: {ctx_json}"
    );

    // ── Step 2: impact ───────────────────────────────────────────────────
    let impact_output = ail()
        .args(["impact", "type.CartItem.price", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let impact_json = parse_json_output(&impact_output);
    assert_eq!(impact_json["status"], "ok", "step 2 (impact) must succeed");
    assert_eq!(
        impact_json["data"]["schema_version"], "1",
        "step 2 (impact): schema_version must be \"1\"; got: {impact_json}"
    );

    // ── Step 3: change ───────────────────────────────────────────────────
    let change_output = ail()
        .args([
            "change",
            "--file",
            path.to_str().expect("path must be UTF-8"),
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let change_json = parse_json_output(&change_output);
    assert_eq!(change_json["status"], "ok", "step 3 (change) must succeed");
    assert_eq!(
        change_json["data"]["schema_version"], "1",
        "step 3 (change): schema_version must be \"1\"; got: {change_json}"
    );
    let change_id = change_json["data"]["canonical_change"]["change_id"]
        .as_str()
        .expect("canonical_change.change_id must be a string")
        .to_string();
    assert_eq!(change_id.len(), 64, "change-id must be 64 hex chars");

    // ── Step 4: verify with persisted changeset ──────────────────────────
    let verify_output = ail()
        .args(["verify", &change_id, "--profile", "dev", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let verify_json = parse_json_output(&verify_output);
    assert_eq!(verify_json["status"], "ok", "step 4 (verify) must succeed");
    assert_eq!(
        verify_json["data"]["schema_version"], "1",
        "step 4 (verify): schema_version must be \"1\"; got: {verify_json}"
    );
    assert!(
        verify_json["data"]["policy_report"].is_object(),
        "step 4 (verify): policy_report must be present; got: {verify_json}"
    );

    // ── Step 5: diff ─────────────────────────────────────────────────────
    let diff_output = ail()
        .args(["diff", "--semantic", "change.add_checkout", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let diff_json = parse_json_output(&diff_output);
    assert_eq!(diff_json["status"], "ok", "step 5 (diff) must succeed");
    assert_eq!(
        diff_json["data"]["schema_version"], "1",
        "step 5 (diff): schema_version must be \"1\"; got: {diff_json}"
    );

    // ── Step 6: apply ────────────────────────────────────────────────────
    let apply_output = ail()
        .args(["apply", &change_id, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let apply_json = parse_json_output(&apply_output);
    assert_eq!(apply_json["status"], "ok", "step 6 (apply) must succeed");
    assert_eq!(
        apply_json["data"]["schema_version"], "1",
        "step 6 (apply): schema_version must be \"1\"; got: {apply_json}"
    );
    assert!(
        apply_json["data"]["new_snapshot_id"].is_string(),
        "step 6 (apply): new_snapshot_id must be present; got: {apply_json}"
    );
}
