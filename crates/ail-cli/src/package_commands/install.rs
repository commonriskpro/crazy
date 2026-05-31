use super::*;

// ── Install ───────────────────────────────────────────────────────────────

pub(super) fn install_package_from_registry(
    store: &StoreHandle,
    name: &str,
    version: &str,
) -> Result<PackageInstallResult, CliError> {
    let (registry, compatibility_metadata) = load_package_registry_with_compatibility(store)?;
    let lookup = trusted_package_lookup(&registry, name, version)?;
    let manifest = &lookup.manifest;
    let hash = manifest
        .blake3_hex()
        .map_err(|e| CliError::Domain(format!("package hash failed: {e}")))?;
    let vr_hash = verification_report_hash_for_manifest(manifest)?;
    let entry = LockfileEntry {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        package_hash: hash,
        trust_level: manifest.trust_level,
        verification_report_hash: vr_hash,
        accepted_assumptions: vec![],
    };
    let mut lockfile = load_package_lockfile(store)?;
    let compatibility_issues =
        package_compatibility_issues_for_install(&lockfile, manifest, &compatibility_metadata)?;
    let blocked_issues = compatibility_issues
        .iter()
        .filter(|issue| issue.status == "blocked")
        .cloned()
        .collect::<Vec<_>>();
    if !blocked_issues.is_empty() {
        return Ok(PackageInstallResult::Blocked(blocked_issues));
    }
    let stored_entry = if let Some(existing) = lockfile
        .entries
        .iter_mut()
        .find(|existing| existing.name == entry.name)
    {
        existing.package_hash = entry.package_hash.clone();
        existing.version = entry.version.clone();
        existing.trust_level = entry.trust_level;
        existing.verification_report_hash = entry.verification_report_hash.clone();
        existing.clone()
    } else {
        lockfile.add(entry.clone());
        entry
    };
    lockfile
        .entries
        .sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
    save_package_lockfile(store, &lockfile)?;

    let lockfile_hash = lockfile
        .blake3_hex()
        .map_err(|e| CliError::Domain(format!("package lock hash failed: {e}")))?;
    let actual = registry
        .all()
        .iter()
        .map(|manifest| {
            manifest
                .blake3_hex()
                .map(|hash| (manifest.name.clone(), manifest.version.clone(), hash))
                .map_err(|e| CliError::Domain(format!("package hash failed: {e}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let actual_refs = actual
        .iter()
        .map(|(name, version, hash)| (name.as_str(), version.as_str(), hash.as_str()))
        .collect::<Vec<_>>();
    let lockfile_reproducibility_issues = lockfile
        .validate_reproducibility(&actual_refs)
        .iter()
        .map(LockfileReproducibilityCliIssue::from_validation_issue)
        .collect::<Vec<_>>();
    let lockfile_reproducibility = if lockfile_reproducibility_issues.is_empty() {
        "ok"
    } else {
        "failed"
    };

    Ok(PackageInstallResult::Installed(Box::new(
        InstalledPackage {
            entry: stored_entry,
            signature_status: lookup.signature_status,
            verification_report: manifest.verification_report.clone(),
            reproducible_evidence: manifest.reproducible_evidence.clone(),
            lockfile_hash,
            installed_package_count: lockfile.len(),
            lockfile_reproducibility,
            lockfile_reproducibility_issues,
            warnings: lookup
                .warning
                .into_iter()
                .chain(
                    compatibility_issues
                        .iter()
                        .filter(|issue| issue.status == "warning")
                        .map(|issue| issue.reason.clone()),
                )
                .collect(),
            compatibility_issues,
        },
    )))
}
