// ── ail-cli integration tests: G31 R2 + T7d ───────────────────────────────
//
// Covers:
//   G31 R2 — extended coverage of all commands (no package/remote)
//   T7d    — LLM agent loop E2E
//
// Shared helpers live in common/mod.rs.

mod common;

use common::{
    ail, compute_sample_change_id, create_sample_change, parse_json_output, sample_acl_path,
};
use predicates::prelude::*;

// ── G31 R2: context with target ──────────────────────────────────────────

/// SC-CTX1: context with target returns hash-bound context slice.
#[test]
fn context_with_target_exits_zero() {
    ail()
        .args(["context", "fn.checkout"])
        .assert()
        .success()
        .stdout(predicate::str::contains("target").or(predicate::str::contains("snapshot")));
}

/// SC-CTX2: context with target --json has context with snapshot_id and hash.
#[test]
fn context_with_target_json_has_context_slice() {
    let output = ail()
        .args(["context", "fn.checkout", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    let ctx = &v["data"]["context"];
    assert!(ctx.is_object(), "data.context must be an object; got: {v}");
    assert!(
        ctx["target"].is_string(),
        "context.target must be a string; got: {ctx}"
    );
    assert!(
        ctx["snapshot_id"].is_string(),
        "context.snapshot_id must be a string; got: {ctx}"
    );
    assert!(
        ctx["snapshot_hash"].is_string(),
        "context.snapshot_hash must be a string; got: {ctx}"
    );
}

// ── G31 R2: impact / callers / effects / proofs ───────────────────────────

/// SC-IMP1: impact returns hash-bound affected_nodes.
#[test]
fn impact_exits_zero_with_snapshot_hash() {
    let output = ail()
        .args(["impact", "type.CartItem.price", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["affected_nodes"].is_array(),
        "affected_nodes must be array; got: {v}"
    );
    assert!(
        v["data"]["snapshot_id"].is_string(),
        "snapshot_id must be string; got: {v}"
    );
    assert!(
        v["data"]["snapshot_hash"].is_string(),
        "snapshot_hash must be string; got: {v}"
    );
}

/// SC-CAL1: callers returns hash-bound callers list.
#[test]
fn callers_exits_zero_with_snapshot_hash() {
    let output = ail()
        .args(["callers", "fn.cart_total", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["callers"].is_array(),
        "callers must be array; got: {v}"
    );
    assert!(
        v["data"]["snapshot_hash"].is_string(),
        "snapshot_hash must be string; got: {v}"
    );
}

/// SC-EFF1: effects returns hash-bound effects list.
#[test]
fn effects_exits_zero_with_snapshot_hash() {
    let output = ail()
        .args(["effects", "module.payment", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["effects"].is_array(),
        "effects must be array; got: {v}"
    );
    assert!(
        v["data"]["snapshot_hash"].is_string(),
        "snapshot_hash must be string; got: {v}"
    );
}

/// SC-PRF1: proofs returns hash-bound proof_obligations.
#[test]
fn proofs_exits_zero_with_snapshot_hash() {
    let output = ail()
        .args(["proofs", "invariant.stock_never_negative", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["proof_obligations"].is_array(),
        "proof_obligations must be array; got: {v}"
    );
    assert!(
        v["data"]["snapshot_hash"].is_string(),
        "snapshot_hash must be string; got: {v}"
    );
}

// ── G31 R2: change with text input ───────────────────────────────────────

/// SC-CH1: change with free-text description creates draft ChangeSet.
#[test]
fn change_text_input_creates_draft() {
    let output = ail()
        .args(["change", "add pure cart_total function", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(
        v["data"]["status"], "draft",
        "change must be draft; got: {v}"
    );
    assert!(
        v["data"]["canonical_change"]["change_id"].is_string(),
        "canonical_change.change_id must be string; got: {v}"
    );
}

/// SC-CH2: change output includes structural_diff preview.
#[test]
fn change_output_includes_structural_diff() {
    let path = sample_acl_path();
    let output = ail()
        .args(["change", "--file", path.to_str().expect("path"), "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    let diff = &v["data"]["structural_diff"];
    assert!(
        diff.is_object(),
        "structural_diff must be an object; got: {v}"
    );
    assert!(
        diff["creates"].is_number(),
        "structural_diff.creates must be a number; got: {diff}"
    );
    assert!(
        diff["modifies"].is_number(),
        "structural_diff.modifies must be a number; got: {diff}"
    );
    assert!(
        diff["deletes"].is_number(),
        "structural_diff.deletes must be a number; got: {diff}"
    );
}

/// SC-CH3: change output includes submitted/parsed/canonical outputs.
#[test]
fn change_output_includes_submitted_parsed_canonical() {
    let path = sample_acl_path();
    let output = ail()
        .args(["change", "--file", path.to_str().expect("path"), "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert!(
        v["data"]["submitted_change"].is_object(),
        "submitted_change must be object; got: {v}"
    );
    assert!(
        v["data"]["parsed_change"].is_object(),
        "parsed_change must be object; got: {v}"
    );
    assert!(
        v["data"]["canonical_change"].is_object(),
        "canonical_change must be object; got: {v}"
    );
}

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

// ── G31 R2: compile --target ──────────────────────────────────────────────

/// SC-CMP1: compile with --target wasm succeeds.
#[test]
fn compile_with_wasm_target_exits_zero() {
    ail()
        .args(["compile", "--target", "wasm", "--profile", "dev"])
        .assert()
        .success()
        .stdout(predicate::str::contains("wasm").or(predicate::str::contains("profile")));
}

/// SC-CMP2: compile --json includes capabilities_manifest, artifact_manifest, compiler_report.
#[test]
fn compile_json_has_manifests_and_report() {
    let output = ail()
        .args(["compile", "--target", "wasm", "--profile", "dev", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["capabilities_manifest"].is_object(),
        "capabilities_manifest must be object; got: {v}"
    );
    assert!(
        v["data"]["artifact_manifest"].is_object(),
        "artifact_manifest must be object; got: {v}"
    );
    assert!(
        v["data"]["compiler_report"].is_object(),
        "compiler_report must be object; got: {v}"
    );
    assert!(
        v["data"]["semantic_source_map"].is_object(),
        "semantic_source_map must be object; got: {v}"
    );
    assert_eq!(v["data"]["artifact_manifest"]["profile"], "dev");
    assert!(
        v["data"]["artifact_manifest"]["capabilities_manifest_hash"].is_array(),
        "artifact_manifest must come from backend sidecar with capabilities_manifest_hash; got: {v}"
    );
    assert!(
        v["data"]["semantic_source_map"]["entries"].is_array(),
        "semantic_source_map must come from backend sidecar entries; got: {v}"
    );
}

/// Feature-H: compile --target wasm --json capabilities_manifest.entries is non-empty.
///
/// The default graph contains `fn.answer` so the compiled WASM artifact must
/// carry at least one entry in capabilities_manifest.entries.
#[test]
fn compile_wasm_json_capabilities_manifest_entries_is_non_empty() {
    let output = ail()
        .args(["compile", "--target", "wasm", "--profile", "dev", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    let entries = v["data"]["capabilities_manifest"]["entries"]
        .as_array()
        .expect("capabilities_manifest.entries must be an array");
    assert!(
        !entries.is_empty(),
        "WASM compile capabilities_manifest.entries must be non-empty for default graph; got: {v}"
    );
}

/// Feature-H: inspect artifact --json capabilities_manifest.entries is non-empty.
///
/// The default graph contains `fn.answer` so the on-demand compiled WASM artifact
/// must carry at least one entry in capabilities_manifest.entries.
#[test]
fn inspect_artifact_capabilities_manifest_entries_is_non_empty() {
    let output = ail()
        .args(["--json", "inspect", "artifact", "program.wasm"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    let entries = v["data"]["capabilities_manifest"]["entries"]
        .as_array()
        .expect("capabilities_manifest.entries must be an array");
    assert!(
        !entries.is_empty(),
        "inspect artifact capabilities_manifest.entries must be non-empty for default graph; got: {v}"
    );
}

/// SC-CMP3: compile with --target native succeeds.
#[test]
fn compile_with_native_target_exits_zero() {
    ail()
        .args(["compile", "--target", "native", "--profile", "prod"])
        .assert()
        .success();
}

/// SC-CMP4: compile --target native --json includes native object fields, not WASM fields.
///
/// Asserts that the native backend is actually reached (emit_native_with_profile):
/// - `object_format` identifies ELF/Mach-O/COFF
/// - `native_bytes` is a non-negative integer (the object file size)
/// - `native_hash` is a non-null string (Blake3 hex of the object bytes)
/// - `compiler_report.stages` includes "emit_native", not "emit_wasm"
/// - the artifact is NOT labelled as a WASM artifact
#[test]
fn compile_native_json_has_object_fields() {
    let output = ail()
        .args([
            "compile",
            "--target",
            "native",
            "--profile",
            "dev",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok", "native compile must succeed; got: {v}");

    // object_format must be a non-empty string (ELF / Mach-O / COFF).
    assert!(
        v["data"]["object_format"].is_string()
            && !v["data"]["object_format"].as_str().unwrap_or("").is_empty(),
        "object_format must be a non-empty string; got: {v}"
    );

    // native_bytes must be a non-negative integer.
    assert!(
        v["data"]["native_bytes"].is_number(),
        "native_bytes must be a number; got: {v}"
    );

    // native_hash must be the sealed native object hash, never the fallback sentinel.
    assert!(
        v["data"]["native_hash"]
            .as_str()
            .is_some_and(|h| h.len() == 64 && h != "<none>"),
        "native_hash must be a 64-char blake3 hex string; got: {v}"
    );

    // compiler_report.stages must include "emit_native".
    let stages = &v["data"]["compiler_report"]["stages"];
    assert!(
        stages
            .as_array()
            .is_some_and(|s| s.iter().any(|e| e.as_str() == Some("emit_native"))),
        "compiler_report.stages must include emit_native; got: {stages}"
    );

    // Must NOT include WASM-specific top-level fields.
    assert!(
        v["data"]["wasm_bytes"].is_null(),
        "native compile must not include wasm_bytes; got: {v}"
    );
    assert!(
        v["data"]["wasm_hash"].is_null(),
        "native compile must not include wasm_hash; got: {v}"
    );

    // capabilities_manifest and artifact_manifest sidecars must be present.
    assert!(
        v["data"]["capabilities_manifest"].is_object(),
        "capabilities_manifest must be object; got: {v}"
    );
    assert!(
        v["data"]["artifact_manifest"].is_object(),
        "artifact_manifest must be object; got: {v}"
    );
}

// ── G31 R2: run with module and replay ───────────────────────────────────

/// SC-RUN1: run with module argument succeeds.
#[test]
fn run_with_module_exits_zero() {
    ail()
        .args(["run", "--profile", "dev", "module.checkout"])
        .assert()
        .success()
        .stdout(predicate::str::contains("PreflightPassed").or(predicate::str::contains("module")));
}

/// SC-RUN2: run --json includes runtime_report, audit_log, capability_call_summary, runtime_check_results.
#[test]
fn run_json_has_full_runtime_report() {
    let output = ail()
        .args(["run", "--profile", "dev", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["runtime_report"].is_object(),
        "runtime_report must be object; got: {v}"
    );
    assert!(
        v["data"]["audit_log"].is_object(),
        "audit_log must be object; got: {v}"
    );
    assert!(
        v["data"]["capability_call_summary"].is_array(),
        "capability_call_summary must be array; got: {v}"
    );
    assert!(
        v["data"]["runtime_check_results"].is_object(),
        "runtime_check_results must be object; got: {v}"
    );
}

/// SC-RUN4: run --json runtime_check_results.artifact_hash is derived from
/// actual preflight (object with "passed" and "hash"), not a hardcoded string.
#[test]
fn run_json_runtime_checks_artifact_hash_is_derived() {
    let output = ail()
        .args(["run", "--profile", "dev", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    let checks = &v["data"]["runtime_check_results"];

    // artifact_hash must be an object (derived), not a plain "ok" string.
    assert!(
        checks["artifact_hash"].is_object(),
        "runtime_check_results.artifact_hash must be an object; got: {checks}"
    );
    assert_eq!(
        checks["artifact_hash"]["passed"], true,
        "artifact_hash.passed must be true after successful preflight; got: {checks}"
    );
    assert!(
        checks["artifact_hash"]["hash"].is_string(),
        "artifact_hash.hash must be a string; got: {checks}"
    );

    // capability_grants must be an object with required/denied counts.
    assert!(
        checks["capability_grants"].is_object(),
        "runtime_check_results.capability_grants must be an object; got: {checks}"
    );
    assert_eq!(
        checks["capability_grants"]["denied"], 0,
        "capability_grants.denied must be 0; got: {checks}"
    );
}

/// SC-RUN5: run --target native exits with code 1 and explicit error message.
///
/// Native linked execution is not supported; the CLI must return a deterministic
/// error rather than silently falling back to WASM execution.
#[test]
fn run_native_target_exits_one_with_explicit_error() {
    ail()
        .args(["run", "--target", "native", "--profile", "dev"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("native"));
}

/// SC-RUN3: run with --replay trace_id includes replay info in JSON.
#[test]
fn run_with_replay_includes_replay_info() {
    let output = ail()
        .args([
            "run",
            "--profile",
            "test",
            "--replay",
            "trace_123",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        !v["data"]["replay"].is_null(),
        "replay info must be present for --replay; got: {v}"
    );
}

// ── G31 R2: init baseline state ───────────────────────────────────────────

/// SC-INIT1: init --json includes branch, policy, runtime_profiles, stdlib_baseline.
#[test]
fn init_json_has_baseline_state() {
    use assert_fs::TempDir;
    let dir = TempDir::new().expect("temp dir");
    let output = ail()
        .args(["init", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["initialized"], true);
    assert_eq!(v["data"]["branch"], "main", "branch must be main; got: {v}");
    assert!(
        v["data"]["policy"].is_string(),
        "policy must be string; got: {v}"
    );
    assert!(
        v["data"]["runtime_profiles"].is_array(),
        "runtime_profiles must be array; got: {v}"
    );
    assert!(
        v["data"]["stdlib_baseline"].is_string(),
        "stdlib_baseline must be string; got: {v}"
    );
    assert!(
        v["data"]["package_lock"].is_string(),
        "package_lock must be string; got: {v}"
    );
    assert!(
        v["data"]["context_indexes"].is_string(),
        "context_indexes must be string; got: {v}"
    );
}

// ── G31 R2: status with all fields ───────────────────────────────────────

/// SC-STAT1: status --json includes verification_state, stale_indexes, runtime_profile_status, package_advisories.
#[test]
fn status_json_has_all_required_fields() {
    let output = ail()
        .args(["status", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["verification_state"].is_string(),
        "verification_state must be string; got: {v}"
    );
    assert!(
        v["data"]["stale_indexes"].is_boolean(),
        "stale_indexes must be boolean; got: {v}"
    );
    assert!(
        v["data"]["runtime_profile_status"].is_string(),
        "runtime_profile_status must be string; got: {v}"
    );
    assert!(
        v["data"]["package_advisories"].is_number(),
        "package_advisories must be number; got: {v}"
    );
}

// ── G31 R2: inspect all types ─────────────────────────────────────────────

/// SC-INS1: inspect node returns edges/effects/capabilities/contracts.
#[test]
fn inspect_node_returns_node_metadata() {
    let output = ail()
        .args(["inspect", "node", "fn.checkout", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["type"], "node");
    assert!(
        v["data"]["edges"].is_array(),
        "edges must be array; got: {v}"
    );
    assert!(
        v["data"]["effects"].is_array(),
        "effects must be array; got: {v}"
    );
    assert!(
        v["data"]["capabilities"].is_array(),
        "capabilities must be array; got: {v}"
    );
    assert!(
        v["data"]["contracts"].is_array(),
        "contracts must be array; got: {v}"
    );
}

/// SC-INS2: inspect report returns status/entries/diagnostics.
#[test]
fn inspect_report_returns_report_metadata() {
    let output = ail()
        .args(["inspect", "report", "ver_123", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["type"], "report");
    assert!(
        v["data"]["entries"].is_array(),
        "entries must be array; got: {v}"
    );
    assert!(
        v["data"]["diagnostics"].is_array(),
        "diagnostics must be array; got: {v}"
    );
}

/// SC-INS2b (Wave 8A): inspect report by hash shows embedded verified_profile.
///
/// Scenario:
///   GIVEN a file-backed project where `ail verify --profile dev` has been run
///   WHEN `ail inspect report <report_hash>` is called with the hash returned by verify
///   THEN the JSON response contains verified_profile = "dev"
///
/// This closes the gap identified in Wave 8A: before this change, loading a
/// report by its content-addressed hash returned `verified_profile: null`
/// because only the sidecar index carried the profile.
#[test]
fn inspect_report_by_hash_shows_embedded_profile() {
    use assert_fs::TempDir;

    let dir = TempDir::new().expect("temp dir");

    // Initialize a file-backed store so the report is persisted.
    ail().arg("init").current_dir(dir.path()).assert().success();

    let change_id = create_sample_change(dir.path());

    // Verify with profile "dev"; capture the report hash from the JSON output.
    let verify_output = ail()
        .args(["verify", &change_id, "--profile", "dev", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let verify_json = parse_json_output(&verify_output);
    let report_hash = verify_json["data"]["verification_report_hash"]
        .as_str()
        .expect("verify must return verification_report_hash");
    assert_eq!(report_hash.len(), 64, "report hash must be 64 hex chars");

    // Inspect by report hash — must show verified_profile = "dev".
    let inspect_output = ail()
        .args(["inspect", "report", report_hash, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&inspect_output);
    assert_eq!(v["status"], "ok", "inspect report by hash must succeed");
    assert_eq!(v["data"]["type"], "report");
    assert_eq!(
        v["data"]["source"], "persisted_by_hash",
        "must load from object store by hash, not sidecar"
    );
    assert_eq!(
        v["data"]["verified_profile"], "dev",
        "inspect report by hash must show verified_profile from embedded field; got: {v}"
    );
}

/// SC-INS2c (Wave 8A): inspect report by change_id still shows profile (sidecar compat).
///
/// Ensures the Wave 7 sidecar path is unaffected: loading by change_id still
/// surfaces the profile even for the updated code path.
#[test]
fn inspect_report_by_change_id_shows_sidecar_profile() {
    use assert_fs::TempDir;

    let dir = TempDir::new().expect("temp dir");
    ail().arg("init").current_dir(dir.path()).assert().success();

    let change_id = create_sample_change(dir.path());

    ail()
        .args(["verify", &change_id, "--profile", "dev"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Inspect by change_id — sidecar path must still return verified_profile.
    let inspect_output = ail()
        .args(["inspect", "report", &change_id, "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&inspect_output);
    assert_eq!(v["status"], "ok");
    assert_eq!(
        v["data"]["source"], "persisted_by_change_id",
        "must load via sidecar when given a change_id"
    );
    assert_eq!(
        v["data"]["verified_profile"], "dev",
        "sidecar-loaded report must still show verified_profile; got: {v}"
    );
}

/// SC-INS3: inspect artifact returns name/hash/profile.
#[test]
fn inspect_artifact_returns_artifact_metadata() {
    let output = ail()
        .args(["inspect", "artifact", "checkout.wasm", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["type"], "artifact");
    assert!(
        v["data"]["name"].is_string(),
        "name must be string; got: {v}"
    );
    assert!(
        v["data"]["semantic_source_map"].is_object(),
        "semantic_source_map must be object; got: {v}"
    );
    assert_eq!(
        v["data"]["capabilities_manifest_source"], "computed_from_wasm_bindings",
        "WASM inspect should label capability manifest source as computed; got: {v}"
    );
}

/// SC-INS4: inspect capability returns provider/granted/assumptions.
/// Feature-F update: inspect capability for an unregistered capability exits 1.
///
/// The old stub returned exit 0 unconditionally. The real implementation
/// queries the package registry and returns NotFound when no registered package
/// exports the requested capability.
/// `payment.charge:PaymentProvider` is not in the default (no-file-store) registry.
#[test]
fn inspect_capability_returns_capability_metadata() {
    ail()
        .args([
            "inspect",
            "capability",
            "payment.charge:PaymentProvider",
            "--json",
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not found"));
}

// ── G31 R2: diff semantic ─────────────────────────────────────────────────

/// SC-DIF1: diff --semantic returns full structural diff with all categories.
#[test]
fn diff_semantic_returns_full_structural_diff() {
    let output = ail()
        .args(["diff", "change.add_checkout", "--semantic", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    let diff = &v["data"]["structural_diff"];
    assert!(diff.is_object(), "structural_diff must be object; got: {v}");
    // Verify all semantic diff categories are present.
    for field in &[
        "creates",
        "modifies",
        "deletes",
        "tombstones",
        "connects",
        "disconnects",
        "exposes",
        "hides",
        "effects_changed",
        "contracts_changed",
        "capabilities_changed",
    ] {
        assert!(
            diff[field].is_array(),
            "structural_diff.{field} must be array; got: {diff}"
        );
    }
}

// ── G31 R2: rollback by change ────────────────────────────────────────────

/// SC-RBK1: rollback with change-id (rollback-by-change) exits 0.
#[test]
fn rollback_by_change_id_exits_zero() {
    let change_id = "ef".repeat(32);
    ail()
        .args(["rollback", &change_id])
        .assert()
        .success()
        .stdout(predicate::str::contains("new snapshot").or(predicate::str::contains("rollback")));
}

/// SC-RBK2: rollback-by-change --json has rollback_type=by_change and reversed_change_id.
#[test]
fn rollback_by_change_json_has_rollback_type() {
    let change_id = "ef".repeat(32);
    let output = ail()
        .args(["rollback", &change_id, "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(
        v["data"]["rollback_type"], "by_change",
        "rollback_type must be by_change; got: {v}"
    );
    assert!(
        v["data"]["reversed_change_id"].is_string(),
        "reversed_change_id must be string; got: {v}"
    );
    assert_eq!(
        v["data"]["history_preserved"], true,
        "history must be preserved; got: {v}"
    );
}

// ── G31 R2: rebase full report ────────────────────────────────────────────

/// SC-REB1: rebase --json has rebase_report with full shape.
#[test]
fn rebase_json_has_rebase_report() {
    let change_id = "ab".repeat(32);
    let onto = "cd".repeat(32);
    let output = ail()
        .args(["rebase", &change_id, "--onto", &onto, "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["rebase_report"].is_object(),
        "rebase_report must be object; got: {v}"
    );
    assert!(
        v["data"]["conflicts"].is_array(),
        "conflicts must be array; got: {v}"
    );
    assert!(
        v["data"]["repair_options"].is_array(),
        "repair_options must be array; got: {v}"
    );
}

// ── G31 R2: merge full conflict workflow ─────────────────────────────────

/// SC-MRG1: merge --json includes rebase_report with conflict info.
#[test]
fn merge_json_has_rebase_report_with_conflicts() {
    let output = ail()
        .args(["merge", "feature.checkout", "--into", "main", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["rebase_report"].is_object(),
        "rebase_report must be object; got: {v}"
    );
    assert!(
        v["data"]["conflicts"].is_array(),
        "conflicts must be array; got: {v}"
    );
    assert!(
        v["data"]["repair_options"].is_array(),
        "repair_options must be array; got: {v}"
    );
}

// ── G31 R2: refactor behavior locks ──────────────────────────────────────

/// SC-REF1: refactor --json has behavior_locks, contracts_preserved, effects_preserved, proofs_to_rerun.
#[test]
fn refactor_json_has_full_behavior_metadata() {
    let output = ail()
        .args(["refactor", "extract-function", "fn.checkout", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["behavior_locks"].is_array(),
        "behavior_locks must be array; got: {v}"
    );
    assert!(
        v["data"]["contracts_preserved"].is_array(),
        "contracts_preserved must be array; got: {v}"
    );
    assert!(
        v["data"]["effects_preserved"].is_array(),
        "effects_preserved must be array; got: {v}"
    );
    assert!(
        v["data"]["proofs_to_rerun"].is_array(),
        "proofs_to_rerun must be array; got: {v}"
    );
    assert_eq!(
        v["data"]["status"], "draft",
        "refactor ChangeSet must be draft; got: {v}"
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

    for required_name in &[
        "graph_integrity",
        "index_freshness",
        "schema_compatibility",
        "artifact_hash_consistency",
        "runtime_profile_validity",
        "package_advisories",
        "assumption_expirations",
    ] {
        assert!(
            check_names.contains(required_name),
            "doctor must include check '{required_name}'; got: {check_names:?}"
        );
    }
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
