use super::common::{ail, create_sample_change, parse_json_output};
use predicates::prelude::*;

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
