#![allow(dead_code)]

use ail_package::{
    CompatibilityClass, Lockfile, LockfileEntry, MigrationRecord, MigrationStep,
    PackageCompatibilityMetadata, PackageDef, PackageKeypair, PackageManifest,
    PackageVerificationReport, ReproducibleBuildEvidence, SecurityAdvisory, SignedPackage,
    TrustLevel, YankRecord,
};
use std::fs;

// ── shared test types ────────────────────────────────────────────────────────

#[derive(serde::Deserialize, serde::Serialize)]
pub struct TestPackageRegistryFile {
    #[serde(default)]
    pub signed_packages: Vec<SignedPackage>,
    #[serde(default)]
    pub legacy_manifests: Vec<PackageManifest>,
    #[serde(default)]
    pub advisories: Vec<SecurityAdvisory>,
    #[serde(default)]
    pub yanked: Vec<YankRecord>,
}

#[derive(serde::Serialize)]
pub struct TestPackageRegistryFileWithCompatibility {
    #[serde(default)]
    pub signed_packages: Vec<SignedPackage>,
    #[serde(default)]
    pub legacy_manifests: Vec<PackageManifest>,
    #[serde(default)]
    pub compatibility_metadata: Vec<PackageCompatibilityMetadata>,
    #[serde(default)]
    pub advisories: Vec<SecurityAdvisory>,
    #[serde(default)]
    pub yanked: Vec<YankRecord>,
}

// ── path helpers ─────────────────────────────────────────────────────────────

pub fn package_registry_path(project_dir: &std::path::Path) -> std::path::PathBuf {
    project_dir
        .join(".ail")
        .join("packages")
        .join("registry.cbor")
}

pub fn package_lockfile_path(project_dir: &std::path::Path) -> std::path::PathBuf {
    project_dir.join(".ail").join("packages").join("lock.cbor")
}

// ── registry I/O helpers ─────────────────────────────────────────────────────

pub fn read_package_registry_file(project_dir: &std::path::Path) -> TestPackageRegistryFile {
    let bytes = fs::read(package_registry_path(project_dir)).expect("registry file must exist");
    ciborium::from_reader(bytes.as_slice()).expect("registry file must decode")
}

pub fn write_package_registry_file(project_dir: &std::path::Path, file: &TestPackageRegistryFile) {
    let path = package_registry_path(project_dir);
    fs::create_dir_all(path.parent().expect("registry path must have parent"))
        .expect("registry directory must be created");
    let mut bytes = Vec::new();
    ciborium::into_writer(file, &mut bytes).expect("registry file must encode");
    fs::write(path, bytes).expect("registry file must be written");
}

pub fn write_package_registry_file_with_compatibility(
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

pub fn write_legacy_package_registry(project_dir: &std::path::Path, manifests: &[PackageManifest]) {
    let path = package_registry_path(project_dir);
    fs::create_dir_all(path.parent().expect("registry path must have parent"))
        .expect("registry directory must be created");
    let mut bytes = Vec::new();
    ciborium::into_writer(manifests, &mut bytes).expect("legacy registry must encode");
    fs::write(path, bytes).expect("legacy registry file must be written");
}

pub fn write_package_lockfile(project_dir: &std::path::Path, lockfile: &Lockfile) {
    let path = package_lockfile_path(project_dir);
    fs::create_dir_all(path.parent().expect("lockfile path must have parent"))
        .expect("package directory must be created");
    let mut bytes = Vec::new();
    ciborium::into_writer(lockfile, &mut bytes).expect("lockfile must encode");
    fs::write(path, bytes).expect("lockfile must be written");
}

// ── manifest / fixture builders ──────────────────────────────────────────────

pub fn signed_test_package(manifest: PackageManifest) -> SignedPackage {
    PackageKeypair::from_bytes(&[17u8; 32])
        .sign_manifest(manifest)
        .expect("test package must sign")
}

pub fn lockfile_for_manifest(manifest: &PackageManifest) -> Lockfile {
    let mut lockfile = Lockfile::new();
    lockfile.add(LockfileEntry {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        requested_version: None,
        package_hash: manifest.blake3_hex().expect("manifest hash must compute"),
        trust_level: manifest.trust_level,
        verification_report_hash: None,
        accepted_assumptions: vec![],
    });
    lockfile
}

pub fn test_package_manifest(
    name: &str,
    version: &str,
    trust_level: TrustLevel,
) -> PackageManifest {
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

pub fn test_reproducible_evidence(name: &str, version: &str) -> ReproducibleBuildEvidence {
    let source_digest = format!("{:b<64}", name.len() + version.len());
    ReproducibleBuildEvidence::new(source_digest, "ail-toolchain-0.1.0", "c".repeat(64))
}

pub fn test_verification_report(package: &str, version: &str) -> PackageVerificationReport {
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

pub fn test_package_manifest_with_report(name: &str, version: &str) -> PackageManifest {
    let mut manifest = test_package_manifest(name, version, TrustLevel::Verified);
    manifest.artifact_hashes = vec![ail_package::ArtifactHashEntry {
        role: "wasm-binary".to_string(),
        hash: "a".repeat(64),
    }];
    manifest.verification_report = Some(test_verification_report(name, version));
    manifest
}

pub fn test_package_manifest_with_full_evidence(name: &str, version: &str) -> PackageManifest {
    let mut manifest = test_package_manifest_with_report(name, version);
    manifest.reproducible_evidence = Some(test_reproducible_evidence(name, version));
    manifest
}

pub fn test_migration(package: &str, from_version: &str, to_version: &str) -> MigrationRecord {
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

pub fn compatibility_metadata(
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
