// ── ail-cli package integration tests ──────────────────────────────────────
//
// Covers all package-related spec scenarios:
//   G31:      package subcommands (add, verify, audit, publish, advisory, yank,
//             install, explain, init)
//   G31 R2:   package full metadata
//   Wave 4G:  reproducible build evidence

mod common;

use ail_package::{
    AdvisorySeverity, CompatibilityClass, Lockfile, LockfileEntry, MigrationRecord, MigrationStep,
    PackageCompatibilityMetadata, PackageDef, PackageKeypair, PackageManifest,
    PackageVerificationReport, ReproducibleBuildEvidence, SecurityAdvisory, SignedPackage,
    TrustLevel, YankRecord,
};
use common::{ail, parse_json_output};
use predicates::prelude::*;
use serde_json::Value;
use std::fs;

// ── package-specific helpers ─────────────────────────────────────────────────

#[derive(serde::Deserialize, serde::Serialize)]
struct TestPackageRegistryFile {
    #[serde(default)]
    signed_packages: Vec<SignedPackage>,
    #[serde(default)]
    legacy_manifests: Vec<PackageManifest>,
    #[serde(default)]
    advisories: Vec<SecurityAdvisory>,
    #[serde(default)]
    yanked: Vec<YankRecord>,
}

#[derive(serde::Serialize)]
struct TestPackageRegistryFileWithCompatibility {
    #[serde(default)]
    signed_packages: Vec<SignedPackage>,
    #[serde(default)]
    legacy_manifests: Vec<PackageManifest>,
    #[serde(default)]
    compatibility_metadata: Vec<PackageCompatibilityMetadata>,
    #[serde(default)]
    advisories: Vec<SecurityAdvisory>,
    #[serde(default)]
    yanked: Vec<YankRecord>,
}

fn package_registry_path(project_dir: &std::path::Path) -> std::path::PathBuf {
    project_dir
        .join(".ail")
        .join("packages")
        .join("registry.cbor")
}

fn package_lockfile_path(project_dir: &std::path::Path) -> std::path::PathBuf {
    project_dir.join(".ail").join("packages").join("lock.cbor")
}

fn read_package_registry_file(project_dir: &std::path::Path) -> TestPackageRegistryFile {
    let bytes = fs::read(package_registry_path(project_dir)).expect("registry file must exist");
    ciborium::from_reader(bytes.as_slice()).expect("registry file must decode")
}

fn write_package_registry_file(project_dir: &std::path::Path, file: &TestPackageRegistryFile) {
    let path = package_registry_path(project_dir);
    fs::create_dir_all(path.parent().expect("registry path must have parent"))
        .expect("registry directory must be created");
    let mut bytes = Vec::new();
    ciborium::into_writer(file, &mut bytes).expect("registry file must encode");
    fs::write(path, bytes).expect("registry file must be written");
}

fn write_package_registry_file_with_compatibility(
    project_dir: &std::path::Path,
    file: &TestPackageRegistryFileWithCompatibility,
) {
    let path = package_registry_path(project_dir);
    fs::create_dir_all(path.parent().expect("registry path must have parent"))
        .expect("registry directory must be created");
    let mut bytes = Vec::new();
    ciborium::into_writer(file, &mut bytes).expect("registry file must encode");
    fs::write(path, bytes).expect("registry file must be written");
}

fn write_legacy_package_registry(project_dir: &std::path::Path, manifests: &[PackageManifest]) {
    let path = package_registry_path(project_dir);
    fs::create_dir_all(path.parent().expect("registry path must have parent"))
        .expect("registry directory must be created");
    let mut bytes = Vec::new();
    ciborium::into_writer(manifests, &mut bytes).expect("legacy registry must encode");
    fs::write(path, bytes).expect("legacy registry file must be written");
}

fn write_package_lockfile(project_dir: &std::path::Path, lockfile: &Lockfile) {
    let path = package_lockfile_path(project_dir);
    fs::create_dir_all(path.parent().expect("lockfile path must have parent"))
        .expect("package directory must be created");
    let mut bytes = Vec::new();
    ciborium::into_writer(lockfile, &mut bytes).expect("lockfile must encode");
    fs::write(path, bytes).expect("lockfile must be written");
}

fn signed_test_package(manifest: PackageManifest) -> SignedPackage {
    PackageKeypair::from_bytes(&[17u8; 32])
        .sign_manifest(manifest)
        .expect("test package must sign")
}

fn lockfile_for_manifest(manifest: &PackageManifest) -> Lockfile {
    let mut lockfile = Lockfile::new();
    lockfile.add(LockfileEntry {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        package_hash: manifest.blake3_hex().expect("manifest hash must compute"),
        trust_level: manifest.trust_level,
        verification_report_hash: None,
        accepted_assumptions: vec![],
    });
    lockfile
}

fn test_package_manifest(name: &str, version: &str, trust_level: TrustLevel) -> PackageManifest {
    PackageManifest::from_def(PackageDef {
        name: name.to_string(),
        version: version.to_string(),
        trust_level,
        required_capabilities: vec![],
        exported_capabilities: vec![],
        assumptions: vec![],
        unsafe_surface: vec![],
        artifact_hashes: vec![],
        build_env_hash: None,
        handlers: vec![],
        contracts: vec![],
        exports: vec![],
        imports: vec![],
        boundaries: vec![],
        license: None,
        provenance: None,
        verification_report: None,
        graph_schema: Some(1),
        core_ir_schema: Some(1),
        // 4G fields
        reproducible_evidence: None,
    })
}

fn test_reproducible_evidence(name: &str, version: &str) -> ReproducibleBuildEvidence {
    let source_digest = format!("{:b<64}", name.len() + version.len());
    ReproducibleBuildEvidence::new(source_digest, "ail-toolchain-0.1.0", "c".repeat(64))
}

fn test_verification_report(package: &str, version: &str) -> PackageVerificationReport {
    PackageVerificationReport {
        package: package.to_string(),
        version: version.to_string(),
        exports_verified: vec!["charge".to_string()],
        effects_declared: vec![],
        assumptions: vec![],
        unsafe_surface: vec![],
        artifact_hashes: vec!["a".repeat(64)],
    }
}

fn test_package_manifest_with_report(name: &str, version: &str) -> PackageManifest {
    let mut manifest = test_package_manifest(name, version, TrustLevel::Verified);
    manifest.artifact_hashes = vec![ail_package::ArtifactHashEntry {
        role: "wasm-binary".to_string(),
        hash: "a".repeat(64),
    }];
    manifest.verification_report = Some(test_verification_report(name, version));
    manifest
}

fn test_package_manifest_with_full_evidence(name: &str, version: &str) -> PackageManifest {
    let mut manifest = test_package_manifest_with_report(name, version);
    manifest.reproducible_evidence = Some(test_reproducible_evidence(name, version));
    manifest
}

fn test_migration(package: &str, from_version: &str, to_version: &str) -> MigrationRecord {
    MigrationRecord {
        package: package.to_string(),
        from_version: from_version.to_string(),
        to_version: to_version.to_string(),
        steps: vec![MigrationStep {
            changed: "capability old.charge".to_string(),
            replacement: "capability new.charge".to_string(),
        }],
    }
}

fn compatibility_metadata(
    package: &str,
    version: &str,
    compatibility: CompatibilityClass,
    migrations: Vec<MigrationRecord>,
) -> PackageCompatibilityMetadata {
    PackageCompatibilityMetadata {
        package: package.to_string(),
        version: version.to_string(),
        compatibility,
        migrations,
    }
}

// ── G31: package ──────────────────────────────────────────────────────────

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

#[test]
fn package_publish_persists_signed_registry_record() {
    let dir = assert_fs::TempDir::new().expect("temp dir must be created");
    ail().arg("init").current_dir(dir.path()).assert().success();

    ail()
        .args(["package", "publish", "--json"])
        .current_dir(dir.path())
        .assert()
        .success();

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
        .stderr(predicate::str::contains("signature verification failed"));
}

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
        .stderr(predicate::str::contains(
            "verified package missing local signature",
        ));
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

// ── G31 R2: package full metadata ────────────────────────────────────────

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

// ── Wave 4G: Reproducible Build Evidence ──────────────────────────────────

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
