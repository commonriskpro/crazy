// ── ail-cli package registry/verification lifecycle tests ───────────────────
//
// G31: publish, advisory, yank, signature integrity, legacy packages,
//      install, lockfile tracking, and verify report-hash lifecycle.

mod common;

use ail_package::{AdvisorySeverity, Lockfile, LockfileEntry, SecurityAdvisory, TrustLevel};
use common::package_helpers::{
    TestPackageRegistryFile, lockfile_for_manifest, package_lockfile_path,
    read_package_registry_file, signed_test_package, test_package_manifest,
    test_package_manifest_with_report, write_legacy_package_registry, write_package_lockfile,
    write_package_registry_file,
};
use common::{ail, parse_json_output};
use predicates::prelude::*;
use serde_json::Value;
use std::fs;

// ── publish ───────────────────────────────────────────────────────────────────

#[test]
fn package_publish_persists_signed_registry_record() {
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
    assert_eq!(v["data"]["published"], true);
    assert_eq!(v["data"]["preflight"], "passed");
    assert_eq!(v["data"]["lockfile_reproducibility"], "ok");
    assert_eq!(v["data"]["locked_package_count"], 0);
    assert_eq!(
        v["data"]["lockfile_reproducibility_issues"],
        Value::Array(vec![])
    );

    let registry = read_package_registry_file(dir.path());
    assert_eq!(registry.signed_packages.len(), 1);
    assert!(registry.legacy_manifests.is_empty());
    registry.signed_packages[0]
        .verify()
        .expect("persisted signed package must verify");
}

#[test]
fn package_advisory_add_persists_local_metadata_and_preserves_signed_records() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    ail()
        .args(["package", "publish"])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = ail()
        .args([
            "package",
            "advisory",
            "add",
            "local.package",
            "<1.0.0",
            "--id",
            "adv_cli_001",
            "--severity",
            "high",
            "--reason",
            "cli recorded advisory",
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["status"], "recorded");
    assert_eq!(v["data"]["scope"], "local");
    assert_eq!(v["data"]["advisory"]["severity"], "high");
    assert_ne!(v["data"]["advisory"]["severity"], "High");

    let registry = read_package_registry_file(dir.path());
    assert_eq!(registry.signed_packages.len(), 1);
    registry.signed_packages[0]
        .verify()
        .expect("signed package must remain verifiable");
    assert_eq!(registry.advisories.len(), 1);
    assert_eq!(registry.advisories[0].id, "adv_cli_001");

    let list_output = ail()
        .args(["package", "advisory", "list", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let list_json = parse_json_output(&list_output);
    assert_eq!(list_json["data"]["count"], 1);
    assert_eq!(list_json["data"]["advisories"][0]["id"], "adv_cli_001");
    assert_eq!(list_json["data"]["advisories"][0]["severity"], "high");
}

#[test]
fn package_advisory_add_rejects_duplicate_id_without_duplicate_record() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    let args = [
        "package",
        "advisory",
        "add",
        "local.package",
        "<1.0.0",
        "--id",
        "adv_cli_duplicate",
        "--severity",
        "high",
        "--reason",
        "first advisory reason",
    ];
    ail().args(args).current_dir(dir.path()).assert().success();

    ail()
        .args([
            "package",
            "advisory",
            "add",
            "other.package",
            "<2.0.0",
            "--id",
            "adv_cli_duplicate",
            "--severity",
            "critical",
            "--reason",
            "duplicate advisory reason",
        ])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "local advisory already exists: adv_cli_duplicate",
        ));

    let registry = read_package_registry_file(dir.path());
    assert_eq!(registry.advisories.len(), 1);
    assert_eq!(registry.advisories[0].id, "adv_cli_duplicate");
    assert_eq!(registry.advisories[0].package, "local.package");
    assert_eq!(registry.advisories[0].reason, "first advisory reason");
}

// ── yank ──────────────────────────────────────────────────────────────────────

#[test]
fn package_yank_persists_local_metadata_and_preserves_advisories() {
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
            "adv_cli_keep",
            "--severity",
            "medium",
            "--reason",
            "keep this advisory",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = ail()
        .args([
            "package",
            "yank",
            "local.package",
            "0.1.0",
            "--reason",
            "bad local release",
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["status"], "recorded");
    assert_eq!(v["data"]["yanked"]["kind"], "yanked");
    assert_eq!(v["data"]["yanked"]["status"], "blocked");

    let registry = read_package_registry_file(dir.path());
    assert_eq!(registry.signed_packages.len(), 1);
    assert_eq!(registry.advisories.len(), 1);
    assert_eq!(registry.yanked.len(), 1);
    assert_eq!(registry.yanked[0].name, "local.package");

    let list_output = ail()
        .args(["package", "yanked", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let list_json = parse_json_output(&list_output);
    assert_eq!(list_json["data"]["count"], 1);
    assert_eq!(list_json["data"]["yanked"][0]["status"], "blocked");
}

#[test]
fn package_yank_repeated_same_package_version_updates_reason_without_duplicate_record() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    ail()
        .args([
            "package",
            "yank",
            "local.package",
            "0.1.0",
            "--reason",
            "initial yank reason",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = ail()
        .args([
            "package",
            "yank",
            "local.package",
            "0.1.0",
            "--reason",
            "updated yank reason",
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["status"], "updated");
    assert_eq!(v["data"]["yanked"]["reason"], "updated yank reason");

    let registry = read_package_registry_file(dir.path());
    assert_eq!(registry.yanked.len(), 1);
    assert_eq!(registry.yanked[0].name, "local.package");
    assert_eq!(registry.yanked[0].version, "0.1.0");
    assert_eq!(registry.yanked[0].reason, "updated yank reason");
}

// ── publish metadata ──────────────────────────────────────────────────────────

#[test]
fn package_publish_reports_missing_verification_report() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    ail()
        .args(["package", "publish"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("verification_report: none"))
        .stdout(predicate::str::contains("verification_report: attached").not());

    let json_dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail()
        .arg("init")
        .current_dir(json_dir.path())
        .assert()
        .success();
    let output = ail()
        .args(["package", "publish", "--json"])
        .current_dir(json_dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["trust"], "verified");
    assert_eq!(v["data"]["verification_report"], Value::Null);
    assert_eq!(v["data"]["verification_report_status"], "none");
}

#[test]
fn package_init_json_manifest_uses_stable_lowercase_trust_level() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    let output = ail()
        .args([
            "package",
            "init",
            "--name",
            "stable.pkg",
            "--version",
            "1.0.0",
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["manifest"]["trust_level"], "verified");
    assert_ne!(v["data"]["manifest"]["trust_level"], "Verified");
}

#[test]
fn package_install_resolver_failure_reports_stable_redacted_diagnostic() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    let output = ail()
        .args([
            "package",
            "install",
            "registry-token-secret@9.9.9",
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("code=install.resolver.not_found"))
        .stderr(predicate::str::contains("category=resolver"))
        .get_output()
        .clone();

    let stdout = std::str::from_utf8(&output.stdout).expect("stdout must be UTF-8");
    let stderr = std::str::from_utf8(&output.stderr).expect("stderr must be UTF-8");
    let v = parse_json_output(&output);
    assert_eq!(v["status"], "error");
    assert_eq!(v["data"]["code"], "install.resolver.not_found");
    assert_eq!(v["data"]["category"], "resolver");
    assert!(!stdout.contains("registry-token-secret"));
    assert!(!stderr.contains("registry-token-secret"));
}

#[test]
fn package_install_advisory_failure_reports_stable_redacted_diagnostic() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest("blocked.pkg", "1.0.0", TrustLevel::Assumed);
    write_package_registry_file(
        dir.path(),
        &TestPackageRegistryFile {
            signed_packages: vec![],
            legacy_manifests: vec![manifest],
            advisories: vec![SecurityAdvisory {
                id: "adv_install_block".to_string(),
                package: "blocked.pkg".to_string(),
                affected_constraint: "<2.0.0".to_string(),
                severity: AdvisorySeverity::High,
                reason: "secret /Users/dev/token-in-advisory must stay local".to_string(),
            }],
            yanked: vec![],
        },
    );

    let output = ail()
        .args(["package", "install", "blocked.pkg@1.0.0", "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("code=install.advisory.blocked"))
        .stderr(predicate::str::contains("category=advisory"))
        .get_output()
        .clone();

    let stdout = std::str::from_utf8(&output.stdout).expect("stdout must be UTF-8");
    let stderr = std::str::from_utf8(&output.stderr).expect("stderr must be UTF-8");
    let v = parse_json_output(&output);
    assert_eq!(v["data"]["code"], "install.advisory.blocked");
    assert_eq!(v["data"]["category"], "advisory");
    assert!(!stdout.contains("token-in-advisory"));
    assert!(!stderr.contains("token-in-advisory"));
}

#[test]
fn package_install_lockfile_failure_reports_stable_redacted_diagnostic() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest("lock.pkg", "1.0.0", TrustLevel::Assumed);
    write_legacy_package_registry(dir.path(), &[manifest]);
    fs::write(
        package_lockfile_path(dir.path()),
        b"not cbor; secret /Users/dev/token-in-lockfile",
    )
    .expect("corrupt lockfile must be written");

    let output = ail()
        .args(["package", "install", "lock.pkg@1.0.0", "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "code=install.lockfile.unavailable",
        ))
        .stderr(predicate::str::contains("category=lockfile"))
        .get_output()
        .clone();

    let stdout = std::str::from_utf8(&output.stdout).expect("stdout must be UTF-8");
    let stderr = std::str::from_utf8(&output.stderr).expect("stderr must be UTF-8");
    let v = parse_json_output(&output);
    assert_eq!(v["data"]["code"], "install.lockfile.unavailable");
    assert_eq!(v["data"]["category"], "lockfile");
    assert!(!stdout.contains("token-in-lockfile"));
    assert!(!stderr.contains("token-in-lockfile"));
}

// ── signature integrity ───────────────────────────────────────────────────────

#[test]
fn package_tampered_signature_fails_verify_and_install() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    ail()
        .args(["package", "publish"])
        .current_dir(dir.path())
        .assert()
        .success();

    let mut registry = read_package_registry_file(dir.path());
    registry.signed_packages[0].sig.signature[0] ^= 0xff;
    write_package_registry_file(dir.path(), &registry);

    ail()
        .args(["package", "verify"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("signature verification failed"));

    ail()
        .args(["package", "install", "local.package@0.1.0"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("code=install.resolver.rejected"))
        .stderr(predicate::str::contains("category=resolver"));
}

// ── legacy unsigned packages ──────────────────────────────────────────────────

#[test]
fn package_legacy_unsigned_install_is_explicit() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest("legacy.pkg", "1.0.0", TrustLevel::Assumed);
    write_legacy_package_registry(dir.path(), &[manifest]);

    let output = ail()
        .args(["package", "install", "legacy.pkg@1.0.0", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["trust"], "assumed");
    assert_eq!(v["data"]["signature_status"], "legacy_unsigned");
    assert_eq!(v["data"]["verification_report"], Value::Null);
    assert_eq!(v["data"]["verification_report_status"], "none");
    assert!(
        v["data"]["warnings"]
            .as_array()
            .is_some_and(|w| !w.is_empty()),
        "legacy unsigned install must surface a warning; got: {v}"
    );
}

#[test]
fn package_add_legacy_unsigned_does_not_claim_accepted_verification_report() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest("legacy.pkg", "1.0.0", TrustLevel::Assumed);
    write_legacy_package_registry(dir.path(), &[manifest]);

    ail()
        .args(["package", "add", "legacy.pkg@1.0.0"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("signature: legacy_unsigned"))
        .stdout(predicate::str::contains("verification_report: none"))
        .stdout(predicate::str::contains("verification_report: accepted").not());
}

#[test]
fn package_legacy_verified_unsigned_install_fails() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest("legacy.verified", "1.0.0", TrustLevel::Verified);
    write_legacy_package_registry(dir.path(), &[manifest]);

    ail()
        .args(["package", "install", "legacy.verified@1.0.0"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("code=install.resolver.rejected"))
        .stderr(predicate::str::contains("category=resolver"));
}

#[test]
fn package_legacy_verified_unsigned_explain_fails() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest("legacy.verified", "1.0.0", TrustLevel::Verified);
    write_legacy_package_registry(dir.path(), &[manifest]);

    ail()
        .args(["package", "explain", "legacy.verified@1.0.0"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "verified package missing local signature",
        ));
}

#[test]
fn package_legacy_unsigned_explain_is_explicit() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest("legacy.pkg", "1.0.0", TrustLevel::Assumed);
    write_legacy_package_registry(dir.path(), &[manifest]);

    let output = ail()
        .args(["package", "explain", "legacy.pkg@1.0.0", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["trust"], "assumed");
    assert_eq!(v["data"]["signature_status"], "legacy_unsigned");
    assert_eq!(v["data"]["verification_report"], Value::Null);
    assert_eq!(v["data"]["verification_report_status"], "none");
    assert!(
        v["data"]["warnings"]
            .as_array()
            .is_some_and(|w| !w.is_empty()),
        "legacy unsigned explain must surface a warning; got: {v}"
    );
}

// ── install / verify happy path ───────────────────────────────────────────────

#[test]
fn package_install_writes_canonical_lockfile_order() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let zeta = test_package_manifest("zeta.pkg", "1.0.0", TrustLevel::Assumed);
    let alpha = test_package_manifest("alpha.pkg", "1.0.0", TrustLevel::Assumed);
    write_legacy_package_registry(dir.path(), &[zeta.clone(), alpha.clone()]);

    ail()
        .args(["package", "install", "zeta.pkg@1.0.0"])
        .current_dir(dir.path())
        .assert()
        .success();
    let install_output = ail()
        .args(["package", "install", "alpha.pkg@1.0.0", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let install_json = parse_json_output(&install_output);

    let lockfile_bytes = fs::read(package_lockfile_path(dir.path())).expect("lockfile must exist");
    let lockfile: Lockfile =
        ciborium::from_reader(lockfile_bytes.as_slice()).expect("decode lockfile");
    let names = lockfile
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec!["alpha.pkg", "zeta.pkg"],
        "package install must persist canonical lockfile order"
    );
    assert_eq!(install_json["data"]["lockfile_reproducibility"], "ok");
    assert_eq!(install_json["data"]["installed_package_count"], 2);
    assert_eq!(
        install_json["data"]["lockfile_reproducibility_issues"],
        Value::Array(vec![])
    );
    assert_eq!(
        install_json["data"]["lockfile_hash"],
        lockfile.blake3_hex().expect("lockfile hash must compute")
    );
}

#[test]
fn package_install_accepts_semver_range_and_locks_resolved_version() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let old = test_package_manifest("range.pkg", "1.2.0", TrustLevel::Assumed);
    let resolved = test_package_manifest("range.pkg", "1.5.0", TrustLevel::Assumed);
    let next_major = test_package_manifest("range.pkg", "2.0.0", TrustLevel::Assumed);
    write_legacy_package_registry(dir.path(), &[old, resolved.clone(), next_major]);

    let output = ail()
        .args(["package", "install", "range.pkg@^1.2", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let install_json = parse_json_output(&output);

    assert_eq!(install_json["data"]["name"], "range.pkg");
    assert_eq!(
        install_json["data"]["version"], "1.5.0",
        "install must lock the highest version satisfying the requested range"
    );

    let lockfile_bytes = fs::read(package_lockfile_path(dir.path())).expect("lockfile must exist");
    let lockfile: Lockfile =
        ciborium::from_reader(lockfile_bytes.as_slice()).expect("decode lockfile");
    assert_eq!(lockfile.entries.len(), 1);
    assert_eq!(lockfile.entries[0].name, "range.pkg");
    assert_eq!(lockfile.entries[0].version, "1.5.0");
    assert_eq!(
        lockfile.entries[0].requested_version.as_deref(),
        Some("^1.2")
    );
    assert_eq!(install_json["data"]["requested_version"], "^1.2");
    assert_eq!(install_json["data"]["resolved_version"], "1.5.0");
    assert_eq!(
        lockfile.entries[0].package_hash,
        resolved
            .blake3_hex()
            .expect("resolved manifest hash must compute")
    );
}

#[test]
fn package_install_rejects_invalid_version_requirement_without_leaking_raw_input() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest("range.pkg", "1.0.0", TrustLevel::Assumed);
    write_legacy_package_registry(dir.path(), &[manifest]);

    let output = ail()
        .args([
            "package",
            "install",
            "range.pkg@customer-secret-not-a-version",
            "--json",
        ])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .get_output()
        .clone();
    let failure_json = parse_json_output(&output);

    assert_eq!(failure_json["status"], "error");
    assert_eq!(
        failure_json["data"]["code"],
        "install.resolver.invalid_requirement"
    );
    assert_eq!(failure_json["data"]["category"], "resolver");
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("customer-secret-not-a-version"),
        "redacted install failure must not leak raw invalid requirement in stderr"
    );
}

#[test]
fn package_publish_rejects_noncanonical_lockfile_preflight_json() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let zeta = test_package_manifest("zeta.pkg", "1.0.0", TrustLevel::Assumed);
    let alpha = test_package_manifest("alpha.pkg", "1.0.0", TrustLevel::Assumed);
    write_package_registry_file(
        dir.path(),
        &TestPackageRegistryFile {
            signed_packages: vec![],
            legacy_manifests: vec![zeta.clone(), alpha.clone()],
            advisories: vec![],
            yanked: vec![],
        },
    );

    let mut lockfile = Lockfile::new();
    lockfile.add(LockfileEntry {
        name: zeta.name.clone(),
        version: zeta.version.clone(),
        requested_version: None,
        package_hash: zeta.blake3_hex().expect("zeta hash must compute"),
        trust_level: zeta.trust_level,
        verification_report_hash: None,
        accepted_assumptions: vec![],
    });
    lockfile.add(LockfileEntry {
        name: alpha.name.clone(),
        version: alpha.version.clone(),
        requested_version: None,
        package_hash: alpha.blake3_hex().expect("alpha hash must compute"),
        trust_level: alpha.trust_level,
        verification_report_hash: None,
        accepted_assumptions: vec![],
    });
    write_package_lockfile(dir.path(), &lockfile);

    let output = ail()
        .args(["package", "publish", "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("package publish preflight failed"))
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["published"], false);
    assert_eq!(v["data"]["preflight"], "failed");
    assert_eq!(v["data"]["lockfile_reproducibility"], "failed");
    assert_eq!(v["data"]["locked_package_count"], 2);
    let issue = &v["data"]["lockfile_reproducibility_issues"][0];
    assert_eq!(issue["kind"], "unstable_entry_order");
    assert_eq!(issue["status"], "blocked");
    assert_eq!(issue["package"], "alpha.pkg");
    assert_eq!(issue["previous_package"], "zeta.pkg");

    let registry = read_package_registry_file(dir.path());
    assert!(
        registry.signed_packages.is_empty(),
        "publish preflight must reject before writing signed packages"
    );
}

#[test]
fn package_verify_rejects_noncanonical_lockfile_order_json() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let zeta = test_package_manifest("zeta.pkg", "1.0.0", TrustLevel::Assumed);
    let alpha = test_package_manifest("alpha.pkg", "1.0.0", TrustLevel::Assumed);
    write_legacy_package_registry(dir.path(), &[zeta.clone(), alpha.clone()]);

    let mut lockfile = Lockfile::new();
    lockfile.add(LockfileEntry {
        name: zeta.name.clone(),
        version: zeta.version.clone(),
        requested_version: None,
        package_hash: zeta.blake3_hex().expect("zeta hash must compute"),
        trust_level: zeta.trust_level,
        verification_report_hash: None,
        accepted_assumptions: vec![],
    });
    lockfile.add(LockfileEntry {
        name: alpha.name.clone(),
        version: alpha.version.clone(),
        requested_version: None,
        package_hash: alpha.blake3_hex().expect("alpha hash must compute"),
        trust_level: alpha.trust_level,
        verification_report_hash: None,
        accepted_assumptions: vec![],
    });
    write_package_lockfile(dir.path(), &lockfile);

    let output = ail()
        .args(["package", "verify", "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("lockfile reproducibility"))
        .get_output()
        .clone();

    let v = parse_json_output(&output);
    assert_eq!(v["data"]["verified"], false);
    assert_eq!(v["data"]["lockfile_reproducibility"], "failed");
    let issue = &v["data"]["lockfile_reproducibility_issues"][0];
    assert_eq!(issue["kind"], "unstable_entry_order");
    assert_eq!(issue["status"], "blocked");
    assert_eq!(issue["package"], "alpha.pkg");
    assert_eq!(issue["version"], "1.0.0");
    assert_eq!(issue["previous_package"], "zeta.pkg");
    assert_eq!(issue["previous_version"], "1.0.0");
}

#[test]
fn package_publish_install_verify_happy_path() {
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
        .success()
        .stdout(predicate::str::contains("signature: signed"));
    let verify_output = ail()
        .args(["package", "verify", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"signature_integrity\":\"ok\""))
        .get_output()
        .clone();

    let v = parse_json_output(&verify_output);
    assert_eq!(v["data"]["packages"][0]["trust_level"], "verified");
}

// ── verification report hash / lockfile tracking ──────────────────────────────

#[test]
fn package_install_stores_verification_report_hash_and_verify_passes() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest_with_report("signed.report", "1.0.0");
    let expected_hash = manifest
        .verification_report
        .as_ref()
        .expect("report must exist")
        .blake3_hex()
        .expect("report hash must compute");
    write_package_registry_file(
        dir.path(),
        &TestPackageRegistryFile {
            signed_packages: vec![signed_test_package(manifest)],
            legacy_manifests: vec![],
            advisories: vec![],
            yanked: vec![],
        },
    );

    let install_output = ail()
        .args(["package", "install", "signed.report@1.0.0", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let install_json = parse_json_output(&install_output);
    assert_eq!(
        install_json["data"]["verification_report_hash"],
        expected_hash
    );

    let lockfile_bytes = fs::read(package_lockfile_path(dir.path())).expect("lockfile must exist");
    let lockfile: Lockfile =
        ciborium::from_reader(lockfile_bytes.as_slice()).expect("decode lockfile");
    assert_eq!(
        lockfile.entries[0].verification_report_hash.as_deref(),
        Some(expected_hash.as_str())
    );

    let verify_output = ail()
        .args(["package", "verify", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let verify_json = parse_json_output(&verify_output);
    assert_eq!(verify_json["data"]["verified"], true);
    assert_eq!(verify_json["data"]["verification_report_integrity"], "ok");
    assert_eq!(
        verify_json["data"]["verification_report_mismatches"],
        Value::Array(vec![])
    );
}

#[test]
fn package_install_updates_existing_legacy_lockfile_entry_with_report_hash() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest_with_report("signed.report", "1.0.0");
    let expected_package_hash = manifest.blake3_hex().expect("manifest hash must compute");
    let expected_report_hash = manifest
        .verification_report
        .as_ref()
        .expect("report must exist")
        .blake3_hex()
        .expect("report hash must compute");
    write_package_registry_file(
        dir.path(),
        &TestPackageRegistryFile {
            signed_packages: vec![signed_test_package(manifest)],
            legacy_manifests: vec![],
            advisories: vec![],
            yanked: vec![],
        },
    );
    let mut legacy_lockfile = Lockfile::new();
    legacy_lockfile.add(LockfileEntry {
        name: "signed.report".to_string(),
        version: "1.0.0".to_string(),
        requested_version: None,
        package_hash: "f".repeat(64),
        trust_level: TrustLevel::Assumed,
        verification_report_hash: None,
        accepted_assumptions: vec!["legacy-assumption".to_string()],
    });
    write_package_lockfile(dir.path(), &legacy_lockfile);

    let install_output = ail()
        .args(["package", "install", "signed.report@1.0.0", "--json"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let install_json = parse_json_output(&install_output);
    assert_eq!(install_json["data"]["package_hash"], expected_package_hash);
    assert_eq!(
        install_json["data"]["verification_report_hash"],
        expected_report_hash
    );

    let lockfile_bytes = fs::read(package_lockfile_path(dir.path())).expect("lockfile must exist");
    let lockfile: Lockfile =
        ciborium::from_reader(lockfile_bytes.as_slice()).expect("decode lockfile");
    assert_eq!(
        lockfile.entries.len(),
        1,
        "install must not duplicate entries"
    );
    assert_eq!(lockfile.entries[0].package_hash, expected_package_hash);
    assert_eq!(lockfile.entries[0].trust_level, TrustLevel::Verified);
    assert_eq!(
        lockfile.entries[0].verification_report_hash.as_deref(),
        Some(expected_report_hash.as_str())
    );
    assert_eq!(
        lockfile.entries[0].accepted_assumptions,
        vec!["legacy-assumption".to_string()]
    );
}

// ── verify report-hash integrity ──────────────────────────────────────────────

#[test]
fn package_verify_reports_verification_report_hash_mismatch() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest_with_report("signed.report", "1.0.0");
    write_package_registry_file(
        dir.path(),
        &TestPackageRegistryFile {
            signed_packages: vec![signed_test_package(manifest.clone())],
            legacy_manifests: vec![],
            advisories: vec![],
            yanked: vec![],
        },
    );
    ail()
        .args(["package", "install", "signed.report@1.0.0"])
        .current_dir(dir.path())
        .assert()
        .success();

    let mut changed = manifest;
    changed
        .verification_report
        .as_mut()
        .expect("report must exist")
        .exports_verified
        .push("refund".to_string());
    write_package_registry_file(
        dir.path(),
        &TestPackageRegistryFile {
            signed_packages: vec![signed_test_package(changed)],
            legacy_manifests: vec![],
            advisories: vec![],
            yanked: vec![],
        },
    );

    let output = ail()
        .args(["package", "verify", "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "verification report hash mismatch",
        ))
        .get_output()
        .clone();
    let v = parse_json_output(&output);
    assert_ne!(v["status"], "ok");
    assert_eq!(v["data"]["verified"], false);
    assert_eq!(v["data"]["verification_report_integrity"], "mismatch");
    assert_eq!(
        v["data"]["verification_report_mismatches"][0]["reason"],
        "hash_mismatch"
    );
    assert!(v["data"]["verification_report_mismatches"][0]["lockfile_hash"].is_string());
    assert!(v["data"]["verification_report_mismatches"][0]["registry_hash"].is_string());
}

#[test]
fn package_verify_reports_missing_registry_verification_report() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest_with_report("signed.report", "1.0.0");
    write_package_registry_file(
        dir.path(),
        &TestPackageRegistryFile {
            signed_packages: vec![signed_test_package(manifest.clone())],
            legacy_manifests: vec![],
            advisories: vec![],
            yanked: vec![],
        },
    );
    ail()
        .args(["package", "install", "signed.report@1.0.0"])
        .current_dir(dir.path())
        .assert()
        .success();

    let mut missing_report = manifest;
    missing_report.verification_report = None;
    write_package_registry_file(
        dir.path(),
        &TestPackageRegistryFile {
            signed_packages: vec![signed_test_package(missing_report)],
            legacy_manifests: vec![],
            advisories: vec![],
            yanked: vec![],
        },
    );

    let output = ail()
        .args(["package", "verify", "--json"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(1)
        .get_output()
        .clone();
    let v = parse_json_output(&output);
    assert_eq!(
        v["data"]["verification_report_mismatches"][0]["reason"],
        "registry_report_missing"
    );
    assert!(v["data"]["verification_report_mismatches"][0]["lockfile_hash"].is_string());
    assert_eq!(
        v["data"]["verification_report_mismatches"][0]["registry_hash"],
        Value::Null
    );
}

#[test]
fn package_verify_reports_legacy_lockfile_missing_report_hash() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();
    let manifest = test_package_manifest_with_report("signed.report", "1.0.0");
    write_package_registry_file(
        dir.path(),
        &TestPackageRegistryFile {
            signed_packages: vec![signed_test_package(manifest.clone())],
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
        .failure()
        .code(1)
        .get_output()
        .clone();
    let v = parse_json_output(&output);
    assert_eq!(v["data"]["verified"], false);
    assert_eq!(
        v["data"]["verification_report_mismatches"][0]["reason"],
        "lockfile_report_hash_missing"
    );
    assert_eq!(
        v["data"]["verification_report_mismatches"][0]["lockfile_hash"],
        Value::Null
    );
    assert!(v["data"]["verification_report_mismatches"][0]["registry_hash"].is_string());
}
