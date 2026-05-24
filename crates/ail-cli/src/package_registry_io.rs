// ── ail-cli::package_registry_io ─────────────────────────────────────────
//
// Local registry and lockfile CBOR read/write helpers, plus the in-process
// `LocalRegistryClient` implementation used by the `ail package` commands.
//
// All disk I/O for package state goes through this module. No business logic
// beyond reading, writing, and the trait impl for local registry operations.

use std::collections::BTreeSet;
use std::path::PathBuf;

use ail_package::{
    PackageCompatibilityMetadata, PackageDef, PackageKeypair, PackageManifest, PackageRegistry,
    PublishRequest, RegistryClient, SearchRequest, SecurityAdvisory, SignedPackage, TrustLevel,
    VerifyOutcome, VerifyRequest, YankRecord,
};

use crate::cli::ail_dir_for_store;
use crate::error::CliError;
use crate::package_output::LocalPackageLookup;
use crate::store::StoreHandle;

// ── Local registry file format ────────────────────────────────────────────

/// On-disk representation of all local package state.
///
/// Fields are individually optional so the CBOR layout evolves gracefully as
/// new fields are added. Legacy registries that stored only a flat
/// `Vec<PackageManifest>` are handled by the fallback path in
/// `load_local_package_registry_file`.
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
pub(crate) struct LocalPackageRegistryFile {
    #[serde(default)]
    pub(crate) signed_packages: Vec<SignedPackage>,
    #[serde(default)]
    pub(crate) legacy_manifests: Vec<PackageManifest>,
    #[serde(default)]
    pub(crate) compatibility_metadata: Vec<PackageCompatibilityMetadata>,
    #[serde(default)]
    pub(crate) advisories: Vec<SecurityAdvisory>,
    #[serde(default)]
    pub(crate) yanked: Vec<YankRecord>,
}

// ── LocalRegistryClient ───────────────────────────────────────────────────

/// In-process registry client backed by the local package registry.
///
/// No network transport is used. All operations are performed against the
/// in-memory `PackageRegistry` that was loaded from the local CBOR file.
pub(crate) struct LocalRegistryClient {
    pub(crate) registry: PackageRegistry,
}

impl RegistryClient for LocalRegistryClient {
    type Error = String;

    fn publish(
        &self,
        request: PublishRequest,
    ) -> Result<ail_package::PublishResponse, Self::Error> {
        request
            .signed_package
            .verify()
            .map_err(|e| format!("signature verification failed: {e}"))?;
        Ok(ail_package::PublishResponse {
            accepted: true,
            error: None,
            log_id: Some(format!(
                "local-log-{}",
                request
                    .signed_package
                    .manifest
                    .blake3_hex()
                    .map_err(|e| e.0)?
            )),
            sequence: Some(self.registry.len() as u64),
        })
    }

    fn fetch(
        &self,
        request: ail_package::FetchRequest,
    ) -> Result<ail_package::FetchResponse, Self::Error> {
        if let Some(signed) = find_signed_package(&self.registry, &request.name, &request.version) {
            signed
                .verify()
                .map_err(|e| format!("signature verification failed: {e}"))?;
            return Ok(ail_package::FetchResponse {
                signed_package: Some(signed.clone()),
                yanked: false,
                error: None,
            });
        }

        let manifest = find_package_manifest(&self.registry, &request.name, &request.version);
        let error = match manifest {
            None => Some(format!(
                "package {} {} not found",
                request.name, request.version
            )),
            Some(manifest) if manifest.trust_level == TrustLevel::Verified => Some(format!(
                "verified package missing local signature: {}@{}",
                manifest.name, manifest.version
            )),
            Some(_) => None,
        };
        Ok(ail_package::FetchResponse {
            signed_package: None,
            yanked: false,
            error,
        })
    }

    fn search(&self, request: SearchRequest) -> Result<ail_package::SearchResponse, Self::Error> {
        let query = request.query.to_lowercase();
        let limit = request.limit.unwrap_or(20) as usize;
        let matching = self
            .registry
            .all()
            .iter()
            .filter(|manifest| manifest.name.to_lowercase().contains(&query))
            .collect::<Vec<_>>();
        let results = matching
            .iter()
            .take(limit)
            .map(|manifest| ail_package::SearchResult {
                name: manifest.name.clone(),
                latest_version: manifest.version.clone(),
                description: manifest.provenance.as_ref().and_then(|p| p.url.clone()),
            })
            .collect::<Vec<_>>();
        Ok(ail_package::SearchResponse {
            truncated: matching.len() > results.len(),
            results,
        })
    }

    fn verify(&self, request: VerifyRequest) -> Result<ail_package::VerifyResponse, Self::Error> {
        let lookup = match trusted_package_lookup(&self.registry, &request.name, &request.version) {
            Ok(lookup) => lookup,
            Err(CliError::NotFound(_)) => {
                return Ok(ail_package::VerifyResponse {
                    outcome: VerifyOutcome::NotFound,
                });
            }
            Err(e) => return Err(e.to_string()),
        };
        let hash = lookup.manifest.blake3_hex().map_err(|e| e.0)?;
        let outcome = if hash == request.expected_hash {
            VerifyOutcome::Ok
        } else {
            VerifyOutcome::HashMismatch {
                registry_hash: hash,
            }
        };
        Ok(ail_package::VerifyResponse { outcome })
    }
}

// ── Directory helpers ─────────────────────────────────────────────────────

pub(crate) fn packages_dir(store: &StoreHandle) -> Result<PathBuf, CliError> {
    Ok(ail_dir_for_store(store)?.join("packages"))
}

pub(crate) fn package_manifest_path(store: &StoreHandle) -> Result<PathBuf, CliError> {
    Ok(ail_dir_for_store(store)?.join("package.cbor"))
}

// ── Registry read helpers ─────────────────────────────────────────────────

pub(crate) fn load_package_registry(store: &StoreHandle) -> Result<PackageRegistry, CliError> {
    load_package_registry_with_advisories(store).map(|(registry, _advisories)| registry)
}

pub(crate) fn load_package_registry_with_compatibility(
    store: &StoreHandle,
) -> Result<(PackageRegistry, Vec<PackageCompatibilityMetadata>), CliError> {
    if !matches!(store, StoreHandle::File { .. }) {
        return Ok((default_memory_package_registry()?, Vec::new()));
    }
    let file = load_local_package_registry_file(store)?;
    let compatibility_metadata = file.compatibility_metadata.clone();
    let (registry, _advisories) = registry_from_file(file)?;
    Ok((registry, compatibility_metadata))
}

pub(crate) fn load_package_registry_with_advisories(
    store: &StoreHandle,
) -> Result<(PackageRegistry, Vec<SecurityAdvisory>), CliError> {
    if !matches!(store, StoreHandle::File { .. }) {
        return Ok((default_memory_package_registry()?, Vec::new()));
    }
    registry_from_file(load_local_package_registry_file(store)?)
}

/// Load the local registry file for read-only access.
///
/// Returns an empty file for non-file stores (memory/Postgres).
pub(crate) fn load_local_package_registry_file_for_read(
    store: &StoreHandle,
) -> Result<LocalPackageRegistryFile, CliError> {
    if !matches!(store, StoreHandle::File { .. }) {
        return Ok(LocalPackageRegistryFile::default());
    }
    load_local_package_registry_file(store)
}

/// Load the local registry file for mutation.
///
/// Returns an error for non-file stores because local metadata management
/// requires an initialized `.ail/` project directory.
pub(crate) fn load_local_package_registry_file_for_update(
    store: &StoreHandle,
) -> Result<LocalPackageRegistryFile, CliError> {
    if !matches!(store, StoreHandle::File { .. }) {
        return Err(CliError::Domain(
            "local package metadata management requires an initialized file project (.ail); run `ail init`"
                .to_string(),
        ));
    }
    load_local_package_registry_file(store)
}

pub(crate) fn load_local_package_registry_file(
    store: &StoreHandle,
) -> Result<LocalPackageRegistryFile, CliError> {
    let path = packages_dir(store)?.join("registry.cbor");
    if !path.exists() {
        return Ok(LocalPackageRegistryFile::default());
    }
    let bytes = std::fs::read(path)?;
    if let Ok(file) = ciborium::from_reader::<LocalPackageRegistryFile, _>(bytes.as_slice()) {
        return Ok(file);
    }
    let legacy_manifests: Vec<PackageManifest> = ciborium::from_reader(bytes.as_slice())
        .map_err(|e| CliError::Domain(format!("package registry decoding failed: {e}")))?;
    Ok(LocalPackageRegistryFile {
        legacy_manifests,
        ..LocalPackageRegistryFile::default()
    })
}

pub(crate) fn registry_from_file(
    file: LocalPackageRegistryFile,
) -> Result<(PackageRegistry, Vec<SecurityAdvisory>), CliError> {
    let mut registry = PackageRegistry::new();
    for signed in file.signed_packages {
        registry
            .register_signed(signed)
            .map_err(|e| CliError::Domain(format!("package signature verification failed: {e}")))?;
    }
    for manifest in file.legacy_manifests {
        registry.register(manifest);
    }
    for yank in file.yanked {
        registry.yank(yank.name, yank.version, yank.reason);
    }
    Ok((registry, file.advisories))
}

// ── Registry write helpers ────────────────────────────────────────────────

pub(crate) fn save_local_package_registry_file(
    store: &StoreHandle,
    file: &LocalPackageRegistryFile,
) -> Result<(), CliError> {
    let dir = packages_dir(store)?;
    std::fs::create_dir_all(&dir)?;
    let mut bytes = Vec::new();
    ciborium::into_writer(file, &mut bytes)
        .map_err(|e| CliError::Domain(format!("package registry encoding failed: {e}")))?;
    std::fs::write(dir.join("registry.cbor"), bytes)?;
    Ok(())
}

pub(crate) fn save_package_registry(
    store: &StoreHandle,
    registry: &PackageRegistry,
) -> Result<(), CliError> {
    if !matches!(store, StoreHandle::File { .. }) {
        let _ = registry;
        return Ok(());
    }
    let dir = packages_dir(store)?;
    std::fs::create_dir_all(&dir)?;
    let existing_file = load_local_package_registry_file(store)?;
    let signed_keys = registry
        .all_signed()
        .iter()
        .map(|signed| {
            (
                signed.manifest.name.clone(),
                signed.manifest.version.clone(),
            )
        })
        .collect::<BTreeSet<_>>();
    let file = LocalPackageRegistryFile {
        signed_packages: registry.all_signed().to_vec(),
        compatibility_metadata: existing_file.compatibility_metadata,
        advisories: existing_file.advisories,
        yanked: registry.yank_records().to_vec(),
        legacy_manifests: registry
            .all()
            .iter()
            .filter(|manifest| {
                !signed_keys.contains(&(manifest.name.clone(), manifest.version.clone()))
            })
            .cloned()
            .collect(),
    };
    let mut bytes = Vec::new();
    ciborium::into_writer(&file, &mut bytes)
        .map_err(|e| CliError::Domain(format!("package registry encoding failed: {e}")))?;
    std::fs::write(dir.join("registry.cbor"), bytes)?;
    Ok(())
}

// ── Lockfile read/write ───────────────────────────────────────────────────

pub(crate) fn load_package_lockfile(
    store: &StoreHandle,
) -> Result<ail_package::Lockfile, CliError> {
    if !matches!(store, StoreHandle::File { .. }) {
        return Ok(ail_package::Lockfile::new());
    }
    let path = packages_dir(store)?.join("lock.cbor");
    if !path.exists() {
        return Ok(ail_package::Lockfile::new());
    }
    let bytes = std::fs::read(path)?;
    ciborium::from_reader(bytes.as_slice())
        .map_err(|e| CliError::Domain(format!("package lock decoding failed: {e}")))
}

pub(crate) fn save_package_lockfile(
    store: &StoreHandle,
    lockfile: &ail_package::Lockfile,
) -> Result<(), CliError> {
    if !matches!(store, StoreHandle::File { .. }) {
        let _ = lockfile;
        return Ok(());
    }
    let dir = packages_dir(store)?;
    std::fs::create_dir_all(&dir)?;
    let mut bytes = Vec::new();
    ciborium::into_writer(lockfile, &mut bytes)
        .map_err(|e| CliError::Domain(format!("package lock encoding failed: {e}")))?;
    std::fs::write(dir.join("lock.cbor"), bytes)?;
    Ok(())
}

// ── Manifest read/write ───────────────────────────────────────────────────

pub(crate) fn save_package_manifest(
    store: &StoreHandle,
    manifest: &PackageManifest,
) -> Result<(), CliError> {
    if !matches!(store, StoreHandle::File { .. }) {
        let _ = manifest;
        return Ok(());
    }
    let path = package_manifest_path(store)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut bytes = Vec::new();
    ciborium::into_writer(manifest, &mut bytes)
        .map_err(|e| CliError::Domain(format!("package manifest encoding failed: {e}")))?;
    std::fs::write(path, bytes)?;
    Ok(())
}

// ── In-memory fixture ─────────────────────────────────────────────────────

/// Build a minimal in-memory package registry for non-file stores.
///
/// Used in tests and memory-store invocations where no `.ail/` directory
/// exists. The fixture contains signed `payments.stripe` entries so that
/// the standard test scenarios succeed without disk I/O.
pub(crate) fn default_memory_package_registry() -> Result<PackageRegistry, CliError> {
    let mut registry = PackageRegistry::new();
    let keypair = PackageKeypair::from_bytes(&[7u8; 32]);
    for (name, version) in [("payments.stripe", "1.2"), ("payments.stripe", "1.2.0")] {
        let manifest = PackageManifest::from_def(PackageDef {
            name: name.to_string(),
            version: version.to_string(),
            trust_level: TrustLevel::Verified,
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
            provenance: Some(ail_package::Provenance::from_url(
                "built-in memory registry fixture",
            )),
            verification_report: None,
            graph_schema: Some(1),
            core_ir_schema: Some(1),
            // 4G fields
            reproducible_evidence: None,
        });
        let signed = keypair
            .sign_manifest(manifest)
            .map_err(|e| CliError::Domain(format!("default package signing failed: {e}")))?;
        registry
            .register_signed(signed)
            .map_err(|e| CliError::Domain(format!("default package signature invalid: {e}")))?;
    }
    Ok(registry)
}

// ── Package lookup helpers ────────────────────────────────────────────────

/// Find the latest (or exact version) manifest in the registry.
pub(crate) fn find_package_manifest<'a>(
    registry: &'a PackageRegistry,
    name: &str,
    version: &str,
) -> Option<&'a PackageManifest> {
    if version == "latest" {
        registry
            .all()
            .iter()
            .rev()
            .find(|manifest| manifest.name == name)
    } else {
        registry.lookup_by_name_version(name, version)
    }
}

/// Find the latest (or exact version) signed package in the registry.
pub(crate) fn find_signed_package<'a>(
    registry: &'a PackageRegistry,
    name: &str,
    version: &str,
) -> Option<&'a SignedPackage> {
    if version == "latest" {
        registry
            .all_signed()
            .iter()
            .rev()
            .find(|signed| signed.manifest.name == name)
    } else {
        registry.lookup_signed_by_name_version(name, version)
    }
}

/// Look up a package with trust verification.
///
/// - Signed packages: signature is verified; returns `signature_status: "signed"`.
/// - Unsigned packages: only allowed for non-Verified trust levels; emits a
///   warning that the trust is not cryptographically verified.
/// - Missing packages: `CliError::NotFound`.
/// - Verified packages without a local signature: `CliError::Domain`.
pub(crate) fn trusted_package_lookup(
    registry: &PackageRegistry,
    name: &str,
    version: &str,
) -> Result<LocalPackageLookup, CliError> {
    if let Some(signed) = find_signed_package(registry, name, version) {
        signed
            .verify()
            .map_err(|e| CliError::Domain(format!("package signature verification failed: {e}")))?;
        return Ok(LocalPackageLookup {
            manifest: signed.manifest.clone(),
            signature_status: "signed",
            warning: None,
        });
    }

    let manifest = find_package_manifest(registry, name, version)
        .ok_or_else(|| CliError::NotFound(format!("package not found: {name}@{version}")))?;
    if manifest.trust_level == TrustLevel::Verified {
        return Err(CliError::Domain(format!(
            "verified package missing local signature: {}@{}",
            manifest.name, manifest.version
        )));
    }
    Ok(LocalPackageLookup {
        manifest: manifest.clone(),
        signature_status: "legacy_unsigned",
        warning: Some(format!(
            "legacy unsigned package metadata for {}@{}; trust is not cryptographically verified",
            manifest.name, manifest.version
        )),
    })
}
