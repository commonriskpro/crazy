use super::*;

// ── Audit ─────────────────────────────────────────────────────────────────

pub(super) fn audit_package_lockfile(
    lockfile: &ail_package::Lockfile,
    registry: &PackageRegistry,
    advisories: &[SecurityAdvisory],
) -> Vec<PackageAuditIssue> {
    let mut issues = Vec::new();

    for entry in &lockfile.entries {
        if let Some(yank) = registry
            .yank_records()
            .iter()
            .find(|yank| yank.name == entry.name && yank.version == entry.version)
        {
            issues.push(PackageAuditIssue::yanked(&entry.name, &entry.version, yank));
        }

        for advisory in AdvisoryChecker::matches(&entry.name, &entry.version, advisories) {
            issues.push(PackageAuditIssue::advisory(
                &entry.name,
                &entry.version,
                advisory,
            ));
        }
    }

    issues
}
