use super::*;

// ── Integrity helpers ─────────────────────────────────────────────────────

pub(super) fn verification_report_hash_for_manifest(
    manifest: &PackageManifest,
) -> Result<Option<String>, CliError> {
    manifest
        .verification_report
        .as_ref()
        .map(|report| {
            report
                .blake3_hex()
                .map_err(|e| CliError::Domain(format!("verification report hash failed: {e}")))
        })
        .transpose()
}
// ── Verify helpers ────────────────────────────────────────────────────────

pub(super) fn verification_report_hash_mismatches(
    lockfile: &ail_package::Lockfile,
    actual_by_package: &BTreeMap<(String, String), RegistryPackageIntegrity>,
) -> Vec<VerificationReportHashMismatch> {
    lockfile
        .entries
        .iter()
        .filter_map(|entry| {
            let actual = actual_by_package.get(&(entry.name.clone(), entry.version.clone()));
            let registry_hash = actual.and_then(|actual| actual.verification_report_hash.clone());
            match (&entry.verification_report_hash, registry_hash) {
                (Some(lockfile_hash), Some(registry_hash)) if lockfile_hash != &registry_hash => {
                    Some(VerificationReportHashMismatch {
                        package: entry.name.clone(),
                        version: entry.version.clone(),
                        reason: "hash_mismatch",
                        lockfile_hash: Some(lockfile_hash.clone()),
                        registry_hash: Some(registry_hash),
                    })
                }
                (Some(lockfile_hash), None) if actual.is_some() => {
                    Some(VerificationReportHashMismatch {
                        package: entry.name.clone(),
                        version: entry.version.clone(),
                        reason: "registry_report_missing",
                        lockfile_hash: Some(lockfile_hash.clone()),
                        registry_hash: None,
                    })
                }
                (None, Some(registry_hash)) => Some(VerificationReportHashMismatch {
                    package: entry.name.clone(),
                    version: entry.version.clone(),
                    reason: "lockfile_report_hash_missing",
                    lockfile_hash: None,
                    registry_hash: Some(registry_hash),
                }),
                _ => None,
            }
        })
        .collect()
}
