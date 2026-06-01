// ── ail-cli package baseline integration tests ──────────────────────────────
//
// G31 smoke tests (SC-PK1 through SC-PK6) and basic JSON shape assertions.
// G31 R2: full metadata field presence.
//
// These tests exercise the public subcommand surface without writing fixture
// files — they rely on the default project state produced by `ail init`.

mod common;

use common::{ail, parse_json_output};
use predicates::prelude::*;
use serde_json::Value;

// ── G31: package subcommand smoke tests ──────────────────────────────────────

/// SC-PK1: package add exits 0 and prints trust/capabilities.
#[test]
fn package_add_exits_zero() {
    ail()
        .args(["package", "add", "payments.stripe@1.2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("trust").or(predicate::str::contains("added")));
}

/// SC-PK2: package verify exits 0.
#[test]
fn package_verify_exits_zero() {
    ail()
        .args(["package", "verify"])
        .assert()
        .success()
        .stdout(predicate::str::contains("verified").or(predicate::str::contains("packages")));
}

/// SC-PK3: package audit exits 0.
#[test]
fn package_audit_exits_zero() {
    ail()
        .args(["package", "audit"])
        .assert()
        .success()
        .stdout(predicate::str::contains("audit").or(predicate::str::contains("advisories")));
}

/// SC-PK4: package publish exits 0.
#[test]
fn package_publish_exits_zero() {
    ail()
        .args(["package", "publish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("publish").or(predicate::str::contains("ok")));
}

/// SC-PK5: package explain exits 0.
#[test]
fn package_explain_exits_zero() {
    ail()
        .args(["package", "explain", "payments.stripe"])
        .assert()
        .success()
        .stdout(predicate::str::contains("package").or(predicate::str::contains("trust")));
}

/// SC-PK6: package add --json produces JSON with package and trust fields.
#[test]
fn package_add_json_has_package_and_trust() {
    let output = ail()
        .args(["package", "add", "payments.stripe@1.2", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["package"].is_string(),
        "data.package must be a string; got: {v}"
    );
    assert!(
        v["data"]["trust"].is_string(),
        "data.trust must be a string; got: {v}"
    );
    assert_eq!(v["data"]["trust"], "verified");
    assert_eq!(v["data"]["verification_report"], Value::Null);
    assert_eq!(v["data"]["verification_report_status"], "none");
}

/// package audit --json produces JSON with advisories array.
#[test]
fn package_audit_json_has_advisories() {
    let output = ail()
        .args(["package", "audit", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["advisories"].is_array(),
        "data.advisories must be an array; got: {v}"
    );
}

/// package verify --json produces JSON with verified field.
#[test]
fn package_verify_json_has_verified() {
    let output = ail()
        .args(["package", "verify", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["verified"].is_boolean(),
        "data.verified must be a boolean; got: {v}"
    );
}

/// package lint --json reports production manifest diagnostics.
#[test]
fn package_lint_json_reports_manifest_issues() {
    let output = ail()
        .args(["package", "lint", "--json"])
        .assert()
        .failure()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "error");
    assert_eq!(v["data"]["error"], "package_lint_failed");
    assert_eq!(v["data"]["passed"], false);
    assert!(
        v["data"]["issues"].is_array(),
        "data.issues must be an array; got: {v}"
    );
    assert!(
        v["data"]["issue_count"].as_u64().unwrap_or_default() > 0,
        "lint should report at least one production manifest issue; got: {v}"
    );
}

/// package init --license --json creates production-clean package metadata.
#[test]
fn package_init_json_accepts_license_metadata() {
    let output = ail()
        .args([
            "package",
            "init",
            "--name",
            "local.package",
            "--version",
            "1.2.3",
            "--license",
            "MIT",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["manifest"]["license"], "MIT");
    assert_eq!(v["data"]["production_lint"], "passed");
    assert_eq!(v["data"]["production_issue_count"], 0);
}

/// package init can attach reproducible-build evidence metadata.
#[test]
fn package_init_json_accepts_reproducible_evidence() {
    let source_digest = "a".repeat(64);
    let recipe_hash = "b".repeat(64);
    let output = ail()
        .args([
            "package",
            "init",
            "--name",
            "local.package",
            "--version",
            "1.2.3",
            "--license",
            "MIT",
            "--source-digest",
            &source_digest,
            "--toolchain-id",
            "ail-toolchain-1",
            "--recipe-hash",
            &recipe_hash,
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["reproducible_evidence_status"], "present");
    assert_eq!(
        v["data"]["manifest"]["reproducible_evidence"]["toolchain_id"],
        "ail-toolchain-1"
    );
    assert!(v["data"]["manifest"]["reproducible_evidence"]["build_inputs_hash"].is_string());
}

/// package explain --json produces JSON with package and capabilities.
#[test]
fn package_explain_json_has_package_and_capabilities() {
    let output = ail()
        .args(["package", "explain", "payments.stripe", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["package"].is_string(),
        "data.package must be a string; got: {v}"
    );
    assert!(
        v["data"]["capabilities"].is_array(),
        "data.capabilities must be an array; got: {v}"
    );
    assert_eq!(v["data"]["trust"], "verified");
    assert_eq!(v["data"]["signature_status"], "signed");
    assert_eq!(v["data"]["verification_report"], Value::Null);
    assert_eq!(v["data"]["verification_report_status"], "none");
}

// ── G31 R2: package full metadata ────────────────────────────────────────────

/// SC-PKG1: package add --json includes trust, verification_report presence,
///          capabilities, assumptions, unsafe_surface, advisories, capabilities_granted=false.
#[test]
fn package_add_json_has_full_metadata() {
    let output = ail()
        .args(["package", "add", "payments.stripe@1.2", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["trust"].is_string(),
        "trust must be string; got: {v}"
    );
    assert_eq!(v["data"]["trust"], "verified");
    assert_eq!(v["data"]["verification_report"], Value::Null);
    assert_eq!(v["data"]["verification_report_status"], "none");
    assert!(
        v["data"]["capabilities"].is_array(),
        "capabilities must be array; got: {v}"
    );
    assert!(
        v["data"]["assumptions"].is_array(),
        "assumptions must be array; got: {v}"
    );
    assert!(
        v["data"]["unsafe_surface"].is_array(),
        "unsafe_surface must be array; got: {v}"
    );
    assert!(
        v["data"]["advisories"].is_array(),
        "advisories must be array; got: {v}"
    );
    assert_eq!(
        v["data"]["capabilities_granted"], false,
        "package install must not grant capabilities; got: {v}"
    );
}

/// SC-PKG2: package audit --json includes packages_checked and assumptions_valid.
#[test]
fn package_audit_json_has_full_audit_fields() {
    let output = ail()
        .args(["package", "audit", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert!(
        v["data"]["packages_checked"].is_number(),
        "packages_checked must be number; got: {v}"
    );
    assert!(
        v["data"]["assumptions_valid"].is_boolean(),
        "assumptions_valid must be boolean; got: {v}"
    );
    assert!(
        v["data"]["unsafe_surface"].is_array(),
        "unsafe_surface must be array; got: {v}"
    );
}
