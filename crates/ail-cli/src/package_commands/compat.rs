use super::*;

// ── Compatibility helpers ─────────────────────────────────────────────────

pub(super) fn package_compatibility_issues_for_install(
    lockfile: &ail_package::Lockfile,
    target_manifest: &PackageManifest,
    metadata: &[PackageCompatibilityMetadata],
) -> Result<Vec<PackageCompatibilityCliIssue>, CliError> {
    let Some(current) = lockfile.entries.iter().find(|entry| {
        entry.name == target_manifest.name && entry.version != target_manifest.version
    }) else {
        return Ok(Vec::new());
    };
    let target_metadata = find_package_compatibility_metadata(
        metadata,
        &target_manifest.name,
        &target_manifest.version,
    );
    match CompatibilityEngine::evaluate_local_upgrade(
        &target_manifest.name,
        &current.version,
        &target_manifest.version,
        target_metadata,
    ) {
        Ok(issues) => Ok(issues
            .into_iter()
            .map(local_compatibility_issue_to_cli)
            .collect()),
        Err(e) => Ok(vec![compatibility_error_to_cli_issue(
            &target_manifest.name,
            &current.version,
            &target_manifest.version,
            e,
        )]),
    }
}

pub(super) fn package_compatibility_issues_for_verify(
    lockfile: &ail_package::Lockfile,
    metadata: &[PackageCompatibilityMetadata],
) -> Result<Vec<PackageCompatibilityCliIssue>, CliError> {
    let mut issues = Vec::new();
    for entry in &lockfile.entries {
        let Some(entry_metadata) =
            find_package_compatibility_metadata(metadata, &entry.name, &entry.version)
        else {
            continue;
        };
        match CompatibilityEngine::evaluate_local_metadata(entry_metadata) {
            Ok(()) => {
                if let Some(migration) = entry_metadata.migrations.first() {
                    issues.push(PackageCompatibilityCliIssue {
                        package: entry.name.clone(),
                        current_version: entry.version.clone(),
                        target_version: entry.version.clone(),
                        kind: "migration",
                        status: "warning",
                        reason: "installed package version carries local migration metadata"
                            .to_string(),
                        migration_id: None,
                        migration_hash: Some(migration.blake3_hex().map_err(|e| {
                            CliError::Domain(format!("migration metadata hash failed: {e}"))
                        })?),
                    });
                }
            }
            Err(e) => issues.push(compatibility_error_to_cli_issue(
                &entry.name,
                &entry.version,
                &entry.version,
                e,
            )),
        }
    }
    Ok(issues)
}

fn find_package_compatibility_metadata<'a>(
    metadata: &'a [PackageCompatibilityMetadata],
    package: &str,
    version: &str,
) -> Option<&'a PackageCompatibilityMetadata> {
    metadata
        .iter()
        .find(|metadata| metadata.package == package && metadata.version == version)
}

fn local_compatibility_issue_to_cli(
    issue: LocalCompatibilityIssue,
) -> PackageCompatibilityCliIssue {
    let status = if issue.migration_hash.is_some() {
        "warning"
    } else {
        "blocked"
    };
    PackageCompatibilityCliIssue {
        package: issue.package,
        current_version: issue.current_version,
        target_version: issue.target_version,
        kind: match issue.kind {
            LocalCompatibilityIssueKind::Compatibility => "compatibility",
            LocalCompatibilityIssueKind::Migration => "migration",
        },
        status,
        reason: issue.reason,
        migration_id: None,
        migration_hash: issue.migration_hash,
    }
}

fn compatibility_error_to_cli_issue(
    package: &str,
    current_version: &str,
    target_version: &str,
    error: CompatibilityError,
) -> PackageCompatibilityCliIssue {
    PackageCompatibilityCliIssue {
        package: package.to_string(),
        current_version: current_version.to_string(),
        target_version: target_version.to_string(),
        kind: compatibility_error_kind(&error),
        status: "blocked",
        reason: format!("local compatibility metadata invalid: {error}").to_ascii_lowercase(),
        migration_id: None,
        migration_hash: None,
    }
}

fn compatibility_error_kind(error: &CompatibilityError) -> &'static str {
    match error {
        CompatibilityError::MajorBumpWithoutMigration
        | CompatibilityError::MigrationPackageMismatch
        | CompatibilityError::MigrationTargetMismatch => "migration",
        CompatibilityError::PatchWithMigration
        | CompatibilityError::InvalidVersion(_)
        | CompatibilityError::MetadataTargetMismatch
        | CompatibilityError::MigrationHashFailed(_) => "compatibility",
    }
}
