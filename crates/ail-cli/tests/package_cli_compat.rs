// ── ail-cli package compat/audit policy + Wave 4G tests ─────────────────────
//
// G31: compatibility upgrade blocking, migration metadata, audit (advisory,
//      yank, severity thresholds).
// Wave 4G: reproducible build evidence surface in install, publish, and verify.

mod common;

use ail_package::{
    AdvisorySeverity, AssumptionState, CompatibilityClass, Lockfile, PackageAssumption,
    SecurityAdvisory, TrustLevel, UnsafeSurfaceEntry, YankRecord,
};
use common::package_helpers::{
    TestPackageRegistryFile, TestPackageRegistryFileWithCompatibility, compatibility_metadata,
    lockfile_for_manifest, package_lockfile_path, signed_test_package, test_migration,
    test_package_manifest, test_package_manifest_with_full_evidence,
    test_package_manifest_with_report, test_reproducible_evidence, write_legacy_package_registry,
    write_package_lockfile, write_package_registry_file,
    write_package_registry_file_with_compatibility,
};
use common::{ail, parse_json_output};
use predicates::prelude::*;
use serde_json::Value;
use std::fs;

// ── G31: compatibility upgrade blocking ───────────────────────────────────────

#[test]
fn package_install_compatible_upgrade_succeeds_without_migration_metadata() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let current = test_package_manifest("compat.pkg", "1.0.0", TrustLevel::Assumed);
    let target = test_package_manifest("compat.pkg", "1.1.0", TrustLevel::Assumed);
    write_package_registry_file(
        dir.path(),
        &TestPackageRegistryFile {
            signed_packages: vec![],
            legacy_manifests: vec![target],
            advisories: vec![],
            yanked: vec![],
        },
    );
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&current));

    let output = ail()
        .args(["package", "install", "compat.pkg@1.1.0", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["installed"], true);
    assert_eq!(v["data"]["version"], "1.1.0");
    assert_eq!(v["data"]["compatibility_issues"], Value::Array(vec![]));

    let lockfile_bytes = fs::read(package_lockfile_path(dir.path())).expect("lockfile must exist");
    let lockfile: Lockfile =
        ciborium::from_reader(lockfile_bytes.as_slice()).expect("decode lockfile");
    assert_eq!(lockfile.entries.len(), 1);
    assert_eq!(lockfile.entries[0].version, "1.1.0");
}

#[test]
fn package_install_breaking_upgrade_without_migration_metadata_is_blocked() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let current = test_package_manifest("breaking.pkg", "1.0.0", TrustLevel::Assumed);
    let target = test_package_manifest("breaking.pkg", "2.0.0", TrustLevel::Assumed);
    write_package_registry_file(
        dir.path(),
        &TestPackageRegistryFile {
            signed_packages: vec![],
            legacy_manifests: vec![target],
            advisories: vec![],
            yanked: vec![],
        },
    );
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&current));

    let output = ail()
        .args(["package", "install", "breaking.pkg@2.0.0", "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("package compatibility blocked"))
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    let issue = &v["data"]["compatibility_issues"][0];
    assert_eq!(issue["package"], "breaking.pkg");
    assert_eq!(issue["current_version"], "1.0.0");
    assert_eq!(issue["target_version"], "2.0.0");
    assert_eq!(issue["kind"], "migration");
    assert_eq!(issue["status"], "blocked");
    assert_eq!(issue["migration_hash"], Value::Null);
}

#[test]
fn package_install_breaking_upgrade_with_migration_metadata_is_allowed_with_warning() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let current = test_package_manifest("migrated.pkg", "1.0.0", TrustLevel::Assumed);
    let target = test_package_manifest("migrated.pkg", "2.0.0", TrustLevel::Assumed);
    write_package_registry_file_with_compatibility(
        dir.path(),
        &TestPackageRegistryFileWithCompatibility {
            signed_packages: vec![],
            legacy_manifests: vec![target],
            compatibility_metadata: vec![compatibility_metadata(
                "migrated.pkg",
                "2.0.0",
                CompatibilityClass::Major,
                vec![test_migration("migrated.pkg", "1.0.0", "2.0.0")],
            )],
            advisories: vec![],
            yanked: vec![],
        },
    );
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&current));

    let output = ail()
        .args(["package", "install", "migrated.pkg@2.0.0", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    let issue = &v["data"]["compatibility_issues"][0];
    assert_eq!(issue["package"], "migrated.pkg");
    assert_eq!(issue["current_version"], "1.0.0");
    assert_eq!(issue["target_version"], "2.0.0");
    assert_eq!(issue["kind"], "migration");
    assert_eq!(issue["status"], "warning");
    assert!(issue["migration_hash"].is_string());
    assert_eq!(issue["migration_id"], Value::Null);
}

#[test]
fn package_install_breaking_upgrade_rejects_migration_for_unrelated_source_version() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let current = test_package_manifest("route.pkg", "1.0.0", TrustLevel::Assumed);
    let target = test_package_manifest("route.pkg", "2.0.0", TrustLevel::Assumed);
    write_package_registry_file_with_compatibility(
        dir.path(),
        &TestPackageRegistryFileWithCompatibility {
            signed_packages: vec![],
            legacy_manifests: vec![target],
            compatibility_metadata: vec![compatibility_metadata(
                "route.pkg",
                "2.0.0",
                CompatibilityClass::Major,
                vec![test_migration("route.pkg", "999.0.0", "2.0.0")],
            )],
            advisories: vec![],
            yanked: vec![],
        },
    );
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&current));

    let output = ail()
        .args(["package", "install", "route.pkg@2.0.0", "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("package compatibility blocked"))
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    let issue = &v["data"]["compatibility_issues"][0];
    assert_eq!(issue["package"], "route.pkg");
    assert_eq!(issue["current_version"], "1.0.0");
    assert_eq!(issue["target_version"], "2.0.0");
    assert_eq!(issue["kind"], "migration");
    assert_eq!(issue["status"], "blocked");
    assert_eq!(issue["migration_hash"], Value::Null);
}

#[test]
fn package_verify_reports_invalid_local_compatibility_metadata() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest("verify.compat", "2.0.0", TrustLevel::Assumed);
    write_package_registry_file_with_compatibility(
        dir.path(),
        &TestPackageRegistryFileWithCompatibility {
            signed_packages: vec![],
            legacy_manifests: vec![manifest.clone()],
            compatibility_metadata: vec![compatibility_metadata(
                "verify.compat",
                "2.0.0",
                CompatibilityClass::Major,
                vec![],
            )],
            advisories: vec![],
            yanked: vec![],
        },
    );
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&manifest));

    let output = ail()
        .args(["package", "verify", "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("compatibility issue"))
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["verified"], false);
    assert_eq!(v["data"]["compatibility_integrity"], "blocked");
    let issue = &v["data"]["compatibility_issues"][0];
    assert_eq!(issue["package"], "verify.compat");
    assert_eq!(issue["current_version"], "2.0.0");
    assert_eq!(issue["target_version"], "2.0.0");
    assert_eq!(issue["kind"], "migration");
    assert_eq!(issue["status"], "blocked");
    assert!(
        issue["reason"]
            .as_str()
            .is_some_and(|reason| reason == reason.to_ascii_lowercase())
    );
}

#[test]
fn package_verify_reports_migration_metadata_warning_json_shape() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest("verify.migration", "2.0.0", TrustLevel::Assumed);
    write_package_registry_file_with_compatibility(
        dir.path(),
        &TestPackageRegistryFileWithCompatibility {
            signed_packages: vec![],
            legacy_manifests: vec![manifest.clone()],
            compatibility_metadata: vec![compatibility_metadata(
                "verify.migration",
                "2.0.0",
                CompatibilityClass::Major,
                vec![test_migration("verify.migration", "1.0.0", "2.0.0")],
            )],
            advisories: vec![],
            yanked: vec![],
        },
    );
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&manifest));

    let output = ail()
        .args(["package", "verify", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["compatibility_integrity"], "warning");
    let issue = &v["data"]["compatibility_issues"][0];
    assert_eq!(issue["package"], "verify.migration");
    assert_eq!(issue["current_version"], "2.0.0");
    assert_eq!(issue["target_version"], "2.0.0");
    assert_eq!(issue["kind"], "migration");
    assert_eq!(issue["status"], "warning");
    assert!(issue["reason"].is_string());
    assert_eq!(issue["migration_id"], Value::Null);
    assert!(issue["migration_hash"].is_string());
}

#[test]
fn package_verify_json_surfaces_assumption_status_per_locked_package() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let mut manifest = test_package_manifest("verify.assumptions", "1.0.0", TrustLevel::Assumed);
    manifest.assumptions = vec![PackageAssumption {
        id: "assume-reviewed-vendor".to_string(),
        claim: "vendor process was reviewed".to_string(),
        boundary: "boundary.vendor".to_string(),
        owner: "security".to_string(),
        expires: None,
        state: AssumptionState::Active,
    }];
    write_legacy_package_registry(dir.path(), std::slice::from_ref(&manifest));
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&manifest));

    let output = ail()
        .args(["package", "verify", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["verified"], true);
    assert_eq!(v["data"]["assumptions_integrity"], "warning");
    assert_eq!(v["data"]["assumptions_valid"], false);
    assert_eq!(
        v["data"]["packages_missing_assumptions"][0]["name"],
        "verify.assumptions"
    );
    assert_eq!(
        v["data"]["packages_missing_assumptions"][0]["version"],
        "1.0.0"
    );
    assert_eq!(
        v["data"]["packages_missing_assumptions"][0]["missing_assumptions"][0],
        "assume-reviewed-vendor"
    );
    assert_eq!(v["data"]["packages"][0]["name"], "verify.assumptions");
    assert_eq!(v["data"]["packages"][0]["assumptions_count"], 1);
    assert_eq!(
        v["data"]["packages"][0]["accepted_assumptions"],
        Value::Array(vec![])
    );
    assert_eq!(
        v["data"]["packages"][0]["missing_assumptions"][0],
        "assume-reviewed-vendor"
    );
    assert_eq!(v["data"]["packages"][0]["assumptions_valid"], false);

    ail()
        .args(["package", "verify"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "WARNING: 1 package(s) missing accepted assumptions",
        ))
        .stdout(predicate::str::contains(
            "verify.assumptions@1.0.0: assume-reviewed-vendor",
        ))
        .stdout(predicate::str::contains("ail package accept-assumption"));
}

// ── G31: audit ────────────────────────────────────────────────────────────────

#[test]
fn package_audit_clean_lockfile_is_explicit_json() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest("clean.pkg", "1.0.0", TrustLevel::Assumed);
    write_legacy_package_registry(dir.path(), std::slice::from_ref(&manifest));
    ail()
        .args(["package", "install", "clean.pkg@1.0.0"])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = ail()
        .args(["package", "audit", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["status"], "clean");
    assert_eq!(v["data"]["packages_checked"], 1);
    assert_eq!(v["data"]["issues"], Value::Array(vec![]));
    assert_eq!(v["data"]["summary"]["blocked"], 0);
    assert_eq!(v["data"]["summary"]["warnings"], 0);
}

#[test]
fn package_audit_json_surfaces_audited_package_metadata() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let mut manifest = test_package_manifest("runtime.files", "1.0.0", TrustLevel::Assumed);
    manifest.required_capabilities = vec!["file.read".to_string()];
    manifest.exported_capabilities = vec!["clock.now".to_string()];
    manifest.assumptions = vec![PackageAssumption {
        id: "assume-sandboxed-path".to_string(),
        claim: "file reads are sandboxed".to_string(),
        boundary: "boundary.local_fs".to_string(),
        owner: "runtime-team".to_string(),
        expires: None,
        state: AssumptionState::Active,
    }];
    manifest.unsafe_surface = vec![UnsafeSurfaceEntry {
        kind: "ffi".to_string(),
        name: "runtime_files_read".to_string(),
        description: "host filesystem bridge".to_string(),
    }];
    write_legacy_package_registry(dir.path(), std::slice::from_ref(&manifest));
    let mut lockfile = lockfile_for_manifest(&manifest);
    lockfile.entries[0].accepted_assumptions = vec!["assume-sandboxed-path".to_string()];
    write_package_lockfile(dir.path(), &lockfile);

    let output = ail()
        .args(["package", "audit", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["packages"][0]["name"], "runtime.files");
    assert_eq!(v["data"]["packages"][0]["capabilities"][0], "file.read");
    assert_eq!(
        v["data"]["packages"][0]["exported_capabilities"][0],
        "clock.now"
    );
    assert_eq!(v["data"]["packages"][0]["assumptions_count"], 1);
    assert_eq!(
        v["data"]["packages"][0]["accepted_assumptions"][0],
        "assume-sandboxed-path"
    );
    assert_eq!(
        v["data"]["packages"][0]["missing_assumptions"],
        Value::Array(vec![])
    );
    assert_eq!(v["data"]["packages"][0]["assumptions_valid"], true);
    assert_eq!(v["data"]["packages"][0]["unsafe_surface_count"], 1);
    assert_eq!(v["data"]["packages"][0]["risk_status"], "clean");
    assert_eq!(v["data"]["assumptions_valid"], true);
    assert_eq!(v["data"]["unsafe_surface"][0]["name"], "runtime_files_read");
}

#[test]
fn package_audit_blocks_unaccepted_package_assumption() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let mut manifest = test_package_manifest("assumed.pkg", "1.0.0", TrustLevel::Assumed);
    manifest.assumptions = vec![PackageAssumption {
        id: "assume-reviewed-vendor".to_string(),
        claim: "vendor process was reviewed".to_string(),
        boundary: "boundary.vendor".to_string(),
        owner: "security".to_string(),
        expires: None,
        state: AssumptionState::Active,
    }];
    write_legacy_package_registry(dir.path(), std::slice::from_ref(&manifest));
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&manifest));

    let output = ail()
        .args(["package", "audit", "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("package audit blocked"))
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["status"], "blocked");
    assert_eq!(v["data"]["assumptions_valid"], false);
    assert_eq!(v["data"]["summary"]["assumptions"], 1);
    assert_eq!(v["data"]["issues"][0]["kind"], "assumption");
    assert_eq!(
        v["data"]["issues"][0]["assumption_id"],
        "assume-reviewed-vendor"
    );
    assert_eq!(
        v["data"]["packages_missing_assumptions"][0]["name"],
        "assumed.pkg"
    );
    assert_eq!(
        v["data"]["packages_missing_assumptions"][0]["version"],
        "1.0.0"
    );
    assert_eq!(
        v["data"]["packages_missing_assumptions"][0]["missing_assumptions"][0],
        "assume-reviewed-vendor"
    );
    assert_eq!(
        v["data"]["packages"][0]["accepted_assumptions"],
        Value::Array(vec![])
    );
    assert_eq!(
        v["data"]["packages"][0]["missing_assumptions"][0],
        "assume-reviewed-vendor"
    );
    assert_eq!(v["data"]["packages"][0]["assumptions_valid"], false);
    assert_eq!(v["data"]["packages"][0]["risk_status"], "blocked");
    assert_eq!(v["data"]["packages"][0]["blocked_issues"], 1);

    ail()
        .args(["package", "audit"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains(
            "remediation: run `ail package accept-assumption`",
        ))
        .stdout(predicate::str::contains(
            "assumed.pkg@1.0.0: assume-reviewed-vendor",
        ))
        .stderr(predicate::str::contains("package audit blocked"));
}

#[test]
fn package_accept_assumption_updates_lockfile_and_unblocks_audit() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let mut manifest = test_package_manifest("assumed.pkg", "1.0.0", TrustLevel::Assumed);
    manifest.assumptions = vec![PackageAssumption {
        id: "assume-reviewed-vendor".to_string(),
        claim: "vendor process was reviewed".to_string(),
        boundary: "boundary.vendor".to_string(),
        owner: "security".to_string(),
        expires: None,
        state: AssumptionState::Active,
    }];
    write_legacy_package_registry(dir.path(), std::slice::from_ref(&manifest));
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&manifest));

    let output = ail()
        .args([
            "package",
            "accept-assumption",
            "assumed.pkg@1.0.0",
            "assume-reviewed-vendor",
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["status"], "accepted");
    assert_eq!(v["data"]["assumption"], "assume-reviewed-vendor");
    let lockfile_bytes = fs::read(package_lockfile_path(dir.path())).expect("lockfile must exist");
    let lockfile: Lockfile =
        ciborium::from_reader(lockfile_bytes.as_slice()).expect("lockfile must decode");
    assert_eq!(
        lockfile.entries[0].accepted_assumptions,
        vec!["assume-reviewed-vendor".to_string()]
    );

    let audit_output = ail()
        .args(["package", "audit", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let audit = parse_json_output(&audit_output);
    assert_eq!(audit["data"]["assumptions_valid"], true);
    assert_eq!(audit["data"]["summary"]["assumptions"], 0);
}

#[test]
fn package_accept_assumption_rejects_undeclared_assumption() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let mut manifest = test_package_manifest("assumed.pkg", "1.0.0", TrustLevel::Assumed);
    manifest.assumptions = vec![PackageAssumption {
        id: "assume-reviewed-vendor".to_string(),
        claim: "vendor process was reviewed".to_string(),
        boundary: "boundary.vendor".to_string(),
        owner: "security".to_string(),
        expires: None,
        state: AssumptionState::Active,
    }];
    write_legacy_package_registry(dir.path(), std::slice::from_ref(&manifest));
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&manifest));

    ail()
        .args([
            "package",
            "accept-assumption",
            "assumed.pkg@1.0.0",
            "unknown-assumption",
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not declare assumption"));
}

#[test]
fn package_audit_signed_registry_advisory_blocks() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest("signed.vuln", "1.0.0", TrustLevel::Verified);
    let signed = signed_test_package(manifest.clone());
    write_package_registry_file(
        dir.path(),
        &TestPackageRegistryFile {
            signed_packages: vec![signed],
            legacy_manifests: vec![],
            advisories: vec![SecurityAdvisory {
                id: "adv_signed_001".to_string(),
                package: "signed.vuln".to_string(),
                affected_constraint: "<1.2.0".to_string(),
                severity: AdvisorySeverity::High,
                reason: "test high severity advisory".to_string(),
            }],
            yanked: vec![],
        },
    );
    ail()
        .args(["package", "install", "signed.vuln@1.0.0"])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = ail()
        .args(["package", "audit", "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("package audit blocked"))
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["status"], "blocked");
    assert_eq!(v["data"]["summary"]["advisories"], 1);
    assert_eq!(v["data"]["summary"]["blocked"], 1);
    let issue = &v["data"]["issues"][0];
    assert_eq!(issue["package"], "signed.vuln");
    assert_eq!(issue["version"], "1.0.0");
    assert_eq!(issue["kind"], "advisory");
    assert_eq!(issue["status"], "blocked");
    assert_eq!(issue["advisory_id"], "adv_signed_001");
    assert_eq!(issue["severity"], "high");
    assert_eq!(issue["affected_range"], "<1.2.0");
}

#[test]
fn package_audit_consumes_cli_created_advisory_metadata() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    ail()
        .args(["package", "publish"])
        .current_dir(dir.path())
        .assert()
        .success();
    ail()
        .args([
            "package",
            "advisory",
            "add",
            "local.package",
            "<1.0.0",
            "--id",
            "adv_cli_audit",
            "--severity",
            "critical",
            "--reason",
            "cli advisory blocks audit",
        ])
        .current_dir(dir.path())
        .assert()
        .success();
    ail()
        .args(["package", "install", "local.package@0.1.0"])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = ail()
        .args(["package", "audit", "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["status"], "blocked");
    assert_eq!(v["data"]["summary"]["advisories"], 1);
    assert_eq!(v["data"]["issues"][0]["kind"], "advisory");
    assert_eq!(v["data"]["issues"][0]["status"], "blocked");
    assert_eq!(v["data"]["issues"][0]["advisory_id"], "adv_cli_audit");
    assert_eq!(v["data"]["issues"][0]["severity"], "critical");
}

#[test]
fn package_audit_consumes_cli_created_yank_metadata() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    ail()
        .args(["package", "publish"])
        .current_dir(dir.path())
        .assert()
        .success();
    ail()
        .args(["package", "install", "local.package@0.1.0"])
        .current_dir(dir.path())
        .assert()
        .success();
    ail()
        .args([
            "package",
            "yank",
            "local.package",
            "0.1.0",
            "--reason",
            "cli yank blocks audit",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = ail()
        .args(["package", "audit", "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["status"], "blocked");
    assert_eq!(v["data"]["summary"]["yanked"], 1);
    assert_eq!(v["data"]["issues"][0]["kind"], "yanked");
    assert_eq!(v["data"]["issues"][0]["status"], "blocked");
    assert_eq!(v["data"]["issues"][0]["reason"], "cli yank blocks audit");
}

#[test]
fn package_audit_critical_advisory_blocks_and_fails() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest("critical.vuln", "3.0.0", TrustLevel::Assumed);
    write_package_registry_file(
        dir.path(),
        &TestPackageRegistryFile {
            signed_packages: vec![],
            legacy_manifests: vec![manifest.clone()],
            advisories: vec![SecurityAdvisory {
                id: "adv_critical_001".to_string(),
                package: "critical.vuln".to_string(),
                affected_constraint: ">=3.0.0, <3.0.1".to_string(),
                severity: AdvisorySeverity::Critical,
                reason: "critical severity advisory".to_string(),
            }],
            yanked: vec![],
        },
    );
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&manifest));

    let output = ail()
        .args(["package", "audit", "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("package audit blocked"))
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["status"], "blocked");
    assert_eq!(v["data"]["summary"]["blocked"], 1);
    assert_eq!(v["data"]["summary"]["warnings"], 0);
    let issue = &v["data"]["issues"][0];
    assert_eq!(issue["advisory_id"], "adv_critical_001");
    assert_eq!(issue["status"], "blocked");
    assert_eq!(issue["severity"], "critical");
}

#[test]
fn package_audit_detects_yanked_lockfile_entry() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest("yanked.pkg", "2.0.0", TrustLevel::Assumed);
    write_package_registry_file(
        dir.path(),
        &TestPackageRegistryFile {
            signed_packages: vec![],
            legacy_manifests: vec![manifest.clone()],
            advisories: vec![],
            yanked: vec![YankRecord {
                name: "yanked.pkg".to_string(),
                version: "2.0.0".to_string(),
                reason: "bad release".to_string(),
            }],
        },
    );
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&manifest));

    let output = ail()
        .args(["package", "audit", "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["status"], "blocked");
    assert_eq!(v["data"]["summary"]["yanked"], 1);
    let issue = &v["data"]["issues"][0];
    assert_eq!(issue["kind"], "yanked");
    assert_eq!(issue["status"], "blocked");
    assert_eq!(issue["package"], "yanked.pkg");
    assert_eq!(issue["reason"], "bad release");
    assert_eq!(v["data"]["packages"][0]["risk_status"], "blocked");
    assert_eq!(v["data"]["packages"][0]["blocked_issues"], 1);
}

#[test]
fn package_audit_low_advisory_warns_without_failing() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest("warn.pkg", "1.1.0", TrustLevel::Assumed);
    write_package_registry_file(
        dir.path(),
        &TestPackageRegistryFile {
            signed_packages: vec![],
            legacy_manifests: vec![manifest.clone()],
            advisories: vec![SecurityAdvisory {
                id: "adv_low_001".to_string(),
                package: "warn.pkg".to_string(),
                affected_constraint: "~1.1.0".to_string(),
                severity: AdvisorySeverity::Low,
                reason: "low severity advisory".to_string(),
            }],
            yanked: vec![],
        },
    );
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&manifest));

    let output = ail()
        .args(["package", "audit", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["status"], "warning");
    assert_eq!(v["data"]["summary"]["warnings"], 1);
    assert_eq!(v["data"]["issues"][0]["status"], "warning");
    assert_eq!(v["data"]["packages"][0]["risk_status"], "warning");
    assert_eq!(v["data"]["packages"][0]["warning_issues"], 1);
}

#[test]
fn package_audit_medium_advisory_warns_without_failing() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest("medium.warn", "2.1.0", TrustLevel::Assumed);
    write_package_registry_file(
        dir.path(),
        &TestPackageRegistryFile {
            signed_packages: vec![],
            legacy_manifests: vec![manifest.clone()],
            advisories: vec![SecurityAdvisory {
                id: "adv_medium_001".to_string(),
                package: "medium.warn".to_string(),
                affected_constraint: "^2.1.0".to_string(),
                severity: AdvisorySeverity::Medium,
                reason: "medium severity advisory".to_string(),
            }],
            yanked: vec![],
        },
    );
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&manifest));

    let output = ail()
        .args(["package", "audit", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["status"], "ok");
    assert_eq!(v["data"]["status"], "warning");
    assert_eq!(v["data"]["summary"]["blocked"], 0);
    assert_eq!(v["data"]["summary"]["warnings"], 1);
    let issue = &v["data"]["issues"][0];
    assert_eq!(issue["advisory_id"], "adv_medium_001");
    assert_eq!(issue["status"], "warning");
    assert_eq!(issue["severity"], "medium");
}

// ── Wave 4G: Reproducible Build Evidence ─────────────────────────────────────

/// SC-4G-1: package install surfaces reproducible_evidence_status = "present"
///           when a package includes full evidence.
///   GIVEN a registry with a signed Verified package with full evidence
///   WHEN `ail package install <pkg>` runs
///   THEN JSON output includes reproducible_evidence_status = "present"
#[test]
fn package_install_surfaces_reproducible_evidence_present() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    let manifest = test_package_manifest_with_full_evidence("repro.pkg", "1.0.0");
    let file = TestPackageRegistryFile {
        signed_packages: vec![signed_test_package(manifest.clone())],
        legacy_manifests: vec![],
        advisories: vec![],
        yanked: vec![],
    };
    write_package_registry_file(dir.path(), &file);
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&manifest));

    let output = ail()
        .args(["package", "install", "repro.pkg@1.0.0", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(
        v["data"]["reproducible_evidence_status"], "present",
        "install of package with full evidence must report status=present; got: {v}"
    );
}

/// SC-4G-2: package install surfaces reproducible_evidence_status = "none"
///           when a package has no evidence.
///   GIVEN a registry with a signed Verified package without evidence
///   WHEN `ail package install <pkg>` runs
///   THEN JSON output includes reproducible_evidence_status = "none"
#[test]
fn package_install_surfaces_reproducible_evidence_none() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    let manifest = test_package_manifest_with_report("norep.pkg", "1.0.0");
    let file = TestPackageRegistryFile {
        signed_packages: vec![signed_test_package(manifest.clone())],
        legacy_manifests: vec![],
        advisories: vec![],
        yanked: vec![],
    };
    write_package_registry_file(dir.path(), &file);
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&manifest));

    let output = ail()
        .args(["package", "install", "norep.pkg@1.0.0", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(
        v["data"]["reproducible_evidence_status"], "none",
        "install of package without evidence must report status=none; got: {v}"
    );
}

/// SC-4G-3: package publish surfaces reproducible_evidence_status in JSON.
///   GIVEN a fresh project (no package manifest → package init creates one)
///   WHEN `ail package publish --json` runs
///   THEN JSON output includes reproducible_evidence_status (either "present" or "none")
///   and the value is a stable lowercase string (no Debug leak).
#[test]
fn package_publish_surfaces_reproducible_evidence_status() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    let output = ail()
        .args(["package", "publish", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    let status = v["data"]["reproducible_evidence_status"]
        .as_str()
        .expect("reproducible_evidence_status must be a string");
    assert!(
        status == "present" || status == "none",
        "reproducible_evidence_status must be 'present' or 'none'; got: {status}"
    );
    // Must not be a Rust Debug representation.
    assert!(
        !status.contains("Some") && !status.contains("None"),
        "reproducible_evidence_status must not leak Rust Debug; got: {status}"
    );
}

/// SC-4G-5: package verify human output warns when verified packages are missing
///   reproducible_evidence.
///   GIVEN a Verified package without reproducible_evidence in the local registry
///   WHEN `ail package verify` runs in human mode (no --json)
///   THEN stdout contains an explicit WARNING about missing reproducible_evidence
///   AND the packages summary line reflects the evidence warning
#[test]
fn package_verify_human_warns_on_missing_reproducible_evidence() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    // A Verified package with no reproducible_evidence — will trigger warning.
    let manifest = test_package_manifest("evidence.missing", "1.0.0", TrustLevel::Verified);
    let signed = signed_test_package(manifest.clone());
    write_package_registry_file(
        dir.path(),
        &TestPackageRegistryFile {
            signed_packages: vec![signed],
            legacy_manifests: vec![],
            advisories: vec![],
            yanked: vec![],
        },
    );
    // lockfile: no verification_report_hash (manifest has no report), so no mismatch.
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&manifest));

    let output = ail()
        .args(["package", "verify"]) // human mode — no --json
        .current_dir(dir.path())
        .assert()
        .success() // still exits 0 (evidence warning is advisory, not a blocker)
        .get_output()
        .clone();

    let stdout = std::str::from_utf8(&output.stdout).expect("stdout must be UTF-8");

    // Human output must contain an explicit WARNING mentioning the gap.
    assert!(
        stdout.contains("WARNING") && stdout.contains("reproducible_evidence"),
        "human output must warn about missing reproducible evidence; got:\n{stdout}"
    );
    // The warning must name the affected package.
    assert!(
        stdout.contains("evidence.missing"),
        "human output WARNING must name the affected package; got:\n{stdout}"
    );
    // The packages summary line must reflect the warning state.
    assert!(
        stdout.contains("reproducible evidence warning"),
        "packages summary must indicate the reproducible evidence warning; got:\n{stdout}"
    );
}

/// SC-4G-6: package verify human output does NOT warn when a Verified package
///   has full reproducible_evidence.
///   GIVEN a Verified package WITH reproducible_evidence (no report — avoids
///   lockfile hash-mismatch in the simplified fixture)
///   WHEN `ail package verify` runs in human mode
///   THEN stdout does not contain "WARNING" and summary says "all verified"
#[test]
fn package_verify_human_no_warning_when_evidence_present() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    // Verified + evidence, but no verification_report so the lockfile doesn't
    // need a report hash (avoids a report-hash mismatch in this fixture).
    let mut manifest = test_package_manifest("evidence.present", "1.0.0", TrustLevel::Verified);
    manifest.reproducible_evidence = Some(test_reproducible_evidence("evidence.present", "1.0.0"));
    let signed = signed_test_package(manifest.clone());
    write_package_registry_file(
        dir.path(),
        &TestPackageRegistryFile {
            signed_packages: vec![signed],
            legacy_manifests: vec![],
            advisories: vec![],
            yanked: vec![],
        },
    );
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&manifest));

    let output = ail()
        .args(["package", "verify"]) // human mode
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = std::str::from_utf8(&output.stdout).expect("stdout must be UTF-8");

    assert!(
        !stdout.contains("WARNING"),
        "human output must NOT warn when evidence is present; got:\n{stdout}"
    );
    assert!(
        stdout.contains("all verified"),
        "packages summary must say 'all verified' when evidence is present; got:\n{stdout}"
    );
    assert!(
        !stdout.contains("reproducible evidence warning"),
        "packages summary must not say 'reproducible evidence warning' when evidence is present; got:\n{stdout}"
    );
}

/// SC-4G-7: package verify JSON includes verified_packages_missing_evidence list
///   when Verified packages lack reproducible_evidence.
///   GIVEN a Verified package without reproducible_evidence
///   WHEN `ail package verify --json` runs
///   THEN verified_packages_missing_evidence contains the package identifier
///   AND reproducible_evidence_integrity is "warning"
///   AND verified is true (backward-compat: evidence is advisory-only)
#[test]
fn package_verify_json_includes_missing_evidence_list() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    let manifest = test_package_manifest("evidence.missing.json", "2.0.0", TrustLevel::Verified);
    let signed = signed_test_package(manifest.clone());
    write_package_registry_file(
        dir.path(),
        &TestPackageRegistryFile {
            signed_packages: vec![signed],
            legacy_manifests: vec![],
            advisories: vec![],
            yanked: vec![],
        },
    );
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&manifest));

    let output = ail()
        .args(["package", "verify", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(
        v["data"]["reproducible_evidence_integrity"], "warning",
        "JSON reproducible_evidence_integrity must be 'warning' when evidence is missing"
    );
    let missing = v["data"]["verified_packages_missing_evidence"]
        .as_array()
        .expect("verified_packages_missing_evidence must be an array");
    assert!(
        missing
            .iter()
            .any(|pkg| pkg.as_str() == Some("evidence.missing.json@2.0.0")),
        "verified_packages_missing_evidence must list the affected package; got: {missing:?}"
    );
    // verified remains true — evidence is advisory-only in package verify.
    assert_eq!(
        v["data"]["verified"], true,
        "verified must remain true; evidence integrity is advisory-only in package verify"
    );
}

/// SC-4G-4: package verify surfaces reproducible_evidence_integrity field.
///   GIVEN a project with a published package
///   WHEN `ail package verify --json` runs
///   THEN JSON output includes reproducible_evidence_integrity (stable lowercase)
#[test]
fn package_verify_surfaces_reproducible_evidence_integrity() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    ail()
        .args(["package", "publish"])
        .current_dir(dir.path())
        .assert()
        .success();

    let manifest = {
        let bytes = fs::read(
            dir.path()
                .join(".ail")
                .join("packages")
                .join("registry.cbor"),
        )
        .expect("registry must exist after publish");
        let file: TestPackageRegistryFile =
            ciborium::from_reader(bytes.as_slice()).expect("registry must decode");
        file.signed_packages
            .into_iter()
            .next()
            .expect("at least one signed package")
            .manifest
    };
    write_package_lockfile(dir.path(), &lockfile_for_manifest(&manifest));

    let output = ail()
        .args(["package", "verify", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    let integrity = v["data"]["reproducible_evidence_integrity"]
        .as_str()
        .expect("reproducible_evidence_integrity must be a string");
    assert!(
        integrity == "ok" || integrity == "warning",
        "reproducible_evidence_integrity must be 'ok' or 'warning'; got: {integrity}"
    );
    // Must not be a Rust Debug representation.
    assert!(
        !integrity.contains("Some") && !integrity.contains("None"),
        "reproducible_evidence_integrity must not leak Rust Debug; got: {integrity}"
    );
}
