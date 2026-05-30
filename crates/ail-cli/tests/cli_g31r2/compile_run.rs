use super::common::{ail, parse_json_output};
use predicates::prelude::*;

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
