// ── ail-cli::package_output ──────────────────────────────────────────────
//
// Human and JSON output helpers for the `ail package` command surface.
//
// All stable JSON field names and human-readable status strings live here
// so that the JSON contract surface is easy to audit in one place.

use ail_package::{
    AdvisorySeverity, LockfileEntry, LockfileValidationIssue, PackageManifest,
    PackageManifestIssue, PackageManifestIssueKind, SecurityAdvisory, YankRecord,
};
use serde_json::{Value, json};

use crate::error::CliError;
use crate::output::{OutputMode, print_error_response};

// ── Output types ─────────────────────────────────────────────────────────

/// Result of a local trusted-package lookup.
pub(crate) struct LocalPackageLookup {
    pub(crate) manifest: PackageManifest,
    pub(crate) signature_status: &'static str,
    pub(crate) warning: Option<String>,
}

/// Data collected after successfully installing a package.
pub(crate) struct InstalledPackage {
    pub(crate) entry: ail_package::LockfileEntry,
    pub(crate) signature_status: &'static str,
    pub(crate) verification_report: Option<ail_package::PackageVerificationReport>,
    /// Locally-recorded reproducible-build evidence from the package manifest.
    pub(crate) reproducible_evidence: Option<ail_package::ReproducibleBuildEvidence>,
    pub(crate) lockfile_hash: String,
    pub(crate) installed_package_count: usize,
    pub(crate) lockfile_reproducibility: &'static str,
    pub(crate) lockfile_reproducibility_issues: Vec<LockfileReproducibilityCliIssue>,
    pub(crate) warnings: Vec<String>,
    pub(crate) compatibility_issues: Vec<PackageCompatibilityCliIssue>,
}

/// Result of an install attempt: either success or a compatibility block.
pub(crate) enum PackageInstallResult {
    Installed(Box<InstalledPackage>),
    Blocked(Vec<PackageCompatibilityCliIssue>),
}

/// Stable, redacted diagnostics for package install failures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PackageInstallFailure {
    pub(crate) code: &'static str,
    pub(crate) category: &'static str,
    pub(crate) message: &'static str,
}

impl PackageInstallFailure {
    pub(crate) fn from_error(error: &CliError) -> Self {
        let rendered = error.to_string().to_ascii_lowercase();
        if rendered.contains("package install blocked by local advisory policy") {
            return Self::advisory_blocked();
        }
        if rendered.contains("package manifest validation failed") {
            return Self::invalid_manifest();
        }
        if rendered.contains("invalid package version requirement") {
            return Self::invalid_requirement();
        }
        if matches!(error, CliError::NotFound(_)) {
            return Self::resolver_not_found();
        }
        if rendered.contains("package lock") || rendered.contains("lockfile") {
            return Self::lockfile_unavailable();
        }
        if rendered.contains("package registry")
            || rendered.contains("signature")
            || rendered.contains("verified package missing local signature")
        {
            return Self::resolver_rejected();
        }
        if matches!(error, CliError::Io(_)) {
            return Self::lockfile_unavailable();
        }
        Self::resolver_rejected()
    }

    pub(crate) fn advisory_blocked() -> Self {
        Self {
            code: "install.advisory.blocked",
            category: "advisory",
            message: "package install blocked by local advisory policy",
        }
    }

    fn resolver_not_found() -> Self {
        Self {
            code: "install.resolver.not_found",
            category: "resolver",
            message: "package resolver failed; requested package was not found",
        }
    }

    fn resolver_rejected() -> Self {
        Self {
            code: "install.resolver.rejected",
            category: "resolver",
            message: "package resolver rejected the package metadata",
        }
    }

    fn invalid_requirement() -> Self {
        Self {
            code: "install.resolver.invalid_requirement",
            category: "resolver",
            message: "package resolver rejected the version requirement",
        }
    }

    fn invalid_manifest() -> Self {
        Self {
            code: "install.manifest.invalid",
            category: "manifest",
            message: "package manifest failed structural validation",
        }
    }

    fn lockfile_unavailable() -> Self {
        Self {
            code: "install.lockfile.unavailable",
            category: "lockfile",
            message: "package lockfile could not be read or written",
        }
    }

    pub(crate) fn to_cli_message(self) -> String {
        format!(
            "package install failed [code={} category={}]: {}",
            self.code, self.category, self.message
        )
    }
}

/// A compatibility issue surfaced to the CLI layer.
#[derive(Clone, Debug)]
pub(crate) struct PackageCompatibilityCliIssue {
    pub(crate) package: String,
    pub(crate) current_version: String,
    pub(crate) target_version: String,
    pub(crate) kind: &'static str,
    pub(crate) status: &'static str,
    pub(crate) reason: String,
    pub(crate) migration_id: Option<String>,
    pub(crate) migration_hash: Option<String>,
}

/// An audit issue for a single lockfile entry.
pub(crate) struct PackageAuditIssue {
    pub(crate) package: String,
    pub(crate) version: String,
    pub(crate) kind: &'static str,
    pub(crate) status: &'static str,
    pub(crate) advisory_id: Option<String>,
    pub(crate) advisory_title: Option<String>,
    pub(crate) severity: Option<String>,
    pub(crate) affected_range: Option<String>,
    pub(crate) reason: Option<String>,
}

/// Cached hash data for a registry package, used during verify.
pub(crate) struct RegistryPackageIntegrity {
    pub(crate) verification_report_hash: Option<String>,
}

/// CLI-facing lockfile reproducibility issue.
pub(crate) struct LockfileReproducibilityCliIssue {
    pub(crate) kind: &'static str,
    pub(crate) status: &'static str,
    pub(crate) package: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) previous_package: Option<String>,
    pub(crate) previous_version: Option<String>,
    pub(crate) expected_hash: Option<String>,
    pub(crate) actual_hash: Option<String>,
    pub(crate) reason: String,
}

impl LockfileReproducibilityCliIssue {
    pub(crate) fn from_validation_issue(issue: &LockfileValidationIssue) -> Self {
        match issue {
            LockfileValidationIssue::UnstableEntryOrder {
                previous_name,
                previous_version,
                name,
                version,
            } => Self {
                kind: "unstable_entry_order",
                status: "blocked",
                package: Some(name.clone()),
                version: Some(version.clone()),
                previous_package: Some(previous_name.clone()),
                previous_version: Some(previous_version.clone()),
                expected_hash: None,
                actual_hash: None,
                reason: format!(
                    "lockfile entries are not in canonical order: {previous_name}@{previous_version} appears before {name}@{version}"
                ),
            },
            LockfileValidationIssue::DuplicatePackageEntry { name, version } => Self {
                kind: "duplicate_lockfile_entry",
                status: "blocked",
                package: Some(name.clone()),
                version: Some(version.clone()),
                previous_package: None,
                previous_version: None,
                expected_hash: None,
                actual_hash: None,
                reason: format!("lockfile contains duplicate entry for {name}@{version}"),
            },
            LockfileValidationIssue::DuplicateActualPackage { name, version } => Self {
                kind: "duplicate_actual_package",
                status: "blocked",
                package: Some(name.clone()),
                version: Some(version.clone()),
                previous_package: None,
                previous_version: None,
                expected_hash: None,
                actual_hash: None,
                reason: format!(
                    "registry produced duplicate actual package coordinate {name}@{version}"
                ),
            },
            LockfileValidationIssue::MissingPackage { name, version } => Self {
                kind: "missing_package",
                status: "blocked",
                package: Some(name.clone()),
                version: Some(version.clone()),
                previous_package: None,
                previous_version: None,
                expected_hash: None,
                actual_hash: None,
                reason: format!("locked package {name}@{version} is missing from registry"),
            },
            LockfileValidationIssue::PackageHashMismatch {
                name,
                version,
                expected,
                actual,
            } => Self {
                kind: "package_hash_mismatch",
                status: "blocked",
                package: Some(name.clone()),
                version: Some(version.clone()),
                previous_package: None,
                previous_version: None,
                expected_hash: Some(expected.clone()),
                actual_hash: Some(actual.clone()),
                reason: format!("locked package {name}@{version} digest differs from registry"),
            },
            LockfileValidationIssue::ArtifactHashMismatch {
                name,
                version,
                expected,
                actual,
            } => Self {
                kind: "artifact_hash_mismatch",
                status: "blocked",
                package: Some(name.clone()),
                version: Some(version.clone()),
                previous_package: None,
                previous_version: None,
                expected_hash: Some(expected.clone()),
                actual_hash: Some(actual.clone()),
                reason: format!(
                    "locked package {name}@{version} artifact evidence differs from registry"
                ),
            },
            LockfileValidationIssue::EmptyPackageHash { name, version } => Self {
                kind: "empty_package_hash",
                status: "blocked",
                package: Some(name.clone()),
                version: Some(version.clone()),
                previous_package: None,
                previous_version: None,
                expected_hash: None,
                actual_hash: None,
                reason: format!("locked package {name}@{version} has an empty package hash"),
            },
            LockfileValidationIssue::EmptyVerificationReportHash { name, version } => Self {
                kind: "empty_verification_report_hash",
                status: "blocked",
                package: Some(name.clone()),
                version: Some(version.clone()),
                previous_package: None,
                previous_version: None,
                expected_hash: None,
                actual_hash: None,
                reason: format!(
                    "locked package {name}@{version} has an empty verification report hash"
                ),
            },
            LockfileValidationIssue::EmptyArtifactRole { name, version } => Self {
                kind: "empty_artifact_role",
                status: "blocked",
                package: Some(name.clone()),
                version: Some(version.clone()),
                previous_package: None,
                previous_version: None,
                expected_hash: None,
                actual_hash: None,
                reason: format!("locked package {name}@{version} has an empty artifact role"),
            },
            LockfileValidationIssue::InvalidArtifactHash {
                name,
                version,
                role,
            } => Self {
                kind: "invalid_artifact_hash",
                status: "blocked",
                package: Some(name.clone()),
                version: Some(version.clone()),
                previous_package: None,
                previous_version: None,
                expected_hash: None,
                actual_hash: None,
                reason: format!(
                    "locked package {name}@{version} has a non-canonical artifact hash for role {role}"
                ),
            },
            LockfileValidationIssue::DuplicateArtifactRole {
                name,
                version,
                role,
            } => Self {
                kind: "duplicate_artifact_role",
                status: "blocked",
                package: Some(name.clone()),
                version: Some(version.clone()),
                previous_package: None,
                previous_version: None,
                expected_hash: None,
                actual_hash: None,
                reason: format!("locked package {name}@{version} repeats artifact role {role}"),
            },
            LockfileValidationIssue::MissingAbiDescriptorArtifact { name, version } => Self {
                kind: "missing_abi_descriptor_artifact",
                status: "blocked",
                package: Some(name.clone()),
                version: Some(version.clone()),
                previous_package: None,
                previous_version: None,
                expected_hash: None,
                actual_hash: None,
                reason: format!(
                    "locked WASM artifact {name}@{version} is missing wasm-abi-descriptor evidence"
                ),
            },
            LockfileValidationIssue::EmptyAcceptedAssumption { name, version } => Self {
                kind: "empty_accepted_assumption",
                status: "blocked",
                package: Some(name.clone()),
                version: Some(version.clone()),
                previous_package: None,
                previous_version: None,
                expected_hash: None,
                actual_hash: None,
                reason: format!("locked package {name}@{version} has an empty accepted assumption"),
            },
            LockfileValidationIssue::DuplicateAcceptedAssumption {
                name,
                version,
                assumption,
            } => Self {
                kind: "duplicate_accepted_assumption",
                status: "blocked",
                package: Some(name.clone()),
                version: Some(version.clone()),
                previous_package: None,
                previous_version: None,
                expected_hash: None,
                actual_hash: None,
                reason: format!(
                    "locked package {name}@{version} repeats accepted assumption {assumption}"
                ),
            },
            LockfileValidationIssue::UnstableAcceptedAssumptionOrder {
                name,
                version,
                previous,
                assumption,
            } => Self {
                kind: "unstable_accepted_assumption_order",
                status: "blocked",
                package: Some(name.clone()),
                version: Some(version.clone()),
                previous_package: None,
                previous_version: None,
                expected_hash: None,
                actual_hash: None,
                reason: format!(
                    "accepted assumptions for {name}@{version} are not canonical: {previous} appears before {assumption}"
                ),
            },
        }
    }

    pub(crate) fn to_json(&self) -> Value {
        json!({
            "kind": self.kind,
            "status": self.status,
            "package": &self.package,
            "version": &self.version,
            "previous_package": &self.previous_package,
            "previous_version": &self.previous_version,
            "expected_hash": &self.expected_hash,
            "actual_hash": &self.actual_hash,
            "reason": &self.reason,
        })
    }

    pub(crate) fn to_human_line(&self) -> String {
        match (&self.package, &self.version) {
            (Some(package), Some(version)) => {
                format!("- {} {}@{}: {}", self.kind, package, version, self.reason)
            }
            _ => format!("- {}: {}", self.kind, self.reason),
        }
    }
}

/// A mismatch between lockfile and registry verification-report hashes.
pub(crate) struct VerificationReportHashMismatch {
    pub(crate) package: String,
    pub(crate) version: String,
    pub(crate) reason: &'static str,
    pub(crate) lockfile_hash: Option<String>,
    pub(crate) registry_hash: Option<String>,
}

impl PackageAuditIssue {
    pub(crate) fn advisory(package: &str, version: &str, advisory: &SecurityAdvisory) -> Self {
        let blocked = advisory.severity >= AdvisorySeverity::High;
        Self {
            package: package.to_string(),
            version: version.to_string(),
            kind: "advisory",
            status: if blocked { "blocked" } else { "warning" },
            advisory_id: Some(advisory.id.clone()),
            advisory_title: Some(advisory.reason.clone()),
            severity: Some(advisory.severity.to_string()),
            affected_range: Some(advisory.affected_constraint.clone()),
            reason: Some(advisory.reason.clone()),
        }
    }

    pub(crate) fn yanked(package: &str, version: &str, yank: &YankRecord) -> Self {
        Self {
            package: package.to_string(),
            version: version.to_string(),
            kind: "yanked",
            status: "blocked",
            advisory_id: None,
            advisory_title: None,
            severity: None,
            affected_range: None,
            reason: Some(yank.reason.clone()),
        }
    }

    pub(crate) fn to_json(&self) -> Value {
        json!({
            "package": &self.package,
            "version": &self.version,
            "kind": self.kind,
            "status": self.status,
            "advisory_id": &self.advisory_id,
            "advisory_title": &self.advisory_title,
            "title": &self.advisory_title,
            "severity": &self.severity,
            "affected_range": &self.affected_range,
            "reason": &self.reason,
        })
    }

    pub(crate) fn to_human_line(&self) -> String {
        match self.kind {
            "advisory" => format!(
                "- advisory {} {}@{} {} {}: {}",
                self.status,
                self.package,
                self.version,
                self.advisory_id.as_deref().unwrap_or("unknown"),
                self.severity.as_deref().unwrap_or("unknown"),
                self.reason.as_deref().unwrap_or("no reason provided")
            ),
            "yanked" => format!(
                "- yanked {} {}@{}: {}",
                self.status,
                self.package,
                self.version,
                self.reason.as_deref().unwrap_or("no reason provided")
            ),
            _ => format!("- {} {}@{}", self.kind, self.package, self.version),
        }
    }
}

// ── Stable status strings ─────────────────────────────────────────────────

/// Stable CLI status string for a verification report attachment.
///
/// Used in both human and JSON output; do not change these string values.
pub(crate) fn verification_report_status(has_report: bool) -> &'static str {
    if has_report { "attached" } else { "none" }
}

/// Stable CLI status string for reproducible-build evidence.
///
/// Returns a stable lowercase string suitable for JSON and human output.
/// This is LOCAL evidence metadata only — no rebuild is performed.
pub(crate) fn reproducible_evidence_status(has_evidence: bool) -> &'static str {
    if has_evidence { "present" } else { "none" }
}

// ── Human output helpers ──────────────────────────────────────────────────

/// Format a non-empty warnings slice for human output (leading newline).
pub(crate) fn format_warnings_for_human(warnings: &[String]) -> String {
    if warnings.is_empty() {
        String::new()
    } else {
        format!("\nwarnings:\n{}", warnings.join("\n"))
    }
}

// ── JSON output helpers ───────────────────────────────────────────────────

pub(crate) fn advisory_to_json(advisory: &SecurityAdvisory) -> Value {
    json!({
        "id": &advisory.id,
        "package": &advisory.package,
        "affected_constraint": &advisory.affected_constraint,
        "severity": advisory.severity.to_string(),
        "reason": &advisory.reason,
        "scope": "local",
    })
}

pub(crate) fn yank_to_json(yank: &YankRecord) -> Value {
    json!({
        "package": &yank.name,
        "name": &yank.name,
        "version": &yank.version,
        "reason": &yank.reason,
        "kind": "yanked",
        "status": "blocked",
        "scope": "local",
    })
}

pub(crate) fn lockfile_entry_to_json(entry: &LockfileEntry) -> Value {
    json!({
        "name": &entry.name,
        "version": &entry.version,
        "resolved_version": &entry.version,
        "requested_version": &entry.requested_version,
        "package_hash": &entry.package_hash,
        "trust_level": entry.trust_level.to_string(),
        "verification_report_hash": &entry.verification_report_hash,
        "artifact_hashes": &entry.artifact_hashes,
        "accepted_assumptions": &entry.accepted_assumptions,
    })
}

pub(crate) fn package_manifest_to_json(manifest: &PackageManifest) -> Result<Value, CliError> {
    let mut value = serde_json::to_value(manifest)
        .map_err(|e| CliError::Domain(format!("package manifest json failed: {e}")))?;
    if let Value::Object(object) = &mut value {
        object.insert(
            "trust_level".to_string(),
            Value::String(manifest.trust_level.to_string()),
        );
    }
    Ok(value)
}

pub(crate) fn package_manifest_issue_to_json(issue: &PackageManifestIssue) -> Value {
    json!({
        "kind": package_manifest_issue_kind(issue.kind.clone()),
        "status": "blocked",
        "descriptor": &issue.descriptor,
        "message": &issue.message,
    })
}

pub(crate) fn package_manifest_issue_to_human_line(issue: &PackageManifestIssue) -> String {
    let mut location = issue.descriptor.path.clone();
    if let Some(index) = issue.descriptor.index {
        location.push_str(&format!("[{index}]"));
    }
    if let Some(duplicate_of) = issue.descriptor.duplicate_of {
        location.push_str(&format!(" duplicates [{duplicate_of}]"));
    }
    format!(
        "- {} at {location}: {}",
        package_manifest_issue_kind(issue.kind.clone()),
        issue.message
    )
}

fn package_manifest_issue_kind(kind: PackageManifestIssueKind) -> &'static str {
    match kind {
        PackageManifestIssueKind::InvalidPackageName => "invalid_package_name",
        PackageManifestIssueKind::InvalidVersion => "invalid_version",
        PackageManifestIssueKind::UnsafeWithoutSurface => "unsafe_without_surface",
        PackageManifestIssueKind::ExportNameEmpty => "export_name_empty",
        PackageManifestIssueKind::HandlerFieldEmpty => "handler_field_empty",
        PackageManifestIssueKind::ImportSourceEmpty => "import_source_empty",
        PackageManifestIssueKind::MissingLicense => "missing_license",
        PackageManifestIssueKind::MissingEntryMetadata => "missing_entry_metadata",
        PackageManifestIssueKind::MissingAbiDescriptorArtifact => "missing_abi_descriptor_artifact",
        PackageManifestIssueKind::ArtifactRoleEmpty => "artifact_role_empty",
        PackageManifestIssueKind::ArtifactHashInvalid => "artifact_hash_invalid",
        PackageManifestIssueKind::DuplicateArtifactRole => "duplicate_artifact_role",
        PackageManifestIssueKind::DuplicateDependency => "duplicate_dependency",
        PackageManifestIssueKind::DuplicateCapability => "duplicate_capability",
        PackageManifestIssueKind::DuplicateExport => "duplicate_export",
    }
}

pub(crate) fn package_compatibility_issue_to_json(issue: &PackageCompatibilityCliIssue) -> Value {
    json!({
        "package": &issue.package,
        "current_version": &issue.current_version,
        "target_version": &issue.target_version,
        "kind": issue.kind,
        "status": issue.status,
        "reason": &issue.reason,
        "migration_id": &issue.migration_id,
        "migration_hash": &issue.migration_hash,
    })
}

pub(crate) fn verification_report_hash_mismatch_to_json(
    mismatch: &VerificationReportHashMismatch,
) -> Value {
    json!({
        "package": &mismatch.package,
        "version": &mismatch.version,
        "reason": mismatch.reason,
        "lockfile_hash": &mismatch.lockfile_hash,
        "registry_hash": &mismatch.registry_hash,
    })
}

/// Emit a JSON error response for a blocked compatibility check.
pub(crate) fn emit_package_compatibility_blocked(
    mode: OutputMode,
    issues: &[PackageCompatibilityCliIssue],
) {
    if mode == OutputMode::Json {
        print_error_response(json!({
            "error": "package_compatibility_blocked",
            "message": format!("package compatibility blocked: {} blocked issue(s)", issues.len()),
            "compatibility_issues": issues.iter().map(package_compatibility_issue_to_json).collect::<Vec<_>>(),
        }));
    }
}

/// Emit a JSON error response for a redacted install failure diagnostic.
pub(crate) fn emit_package_install_failure(mode: OutputMode, failure: PackageInstallFailure) {
    if mode == OutputMode::Json {
        print_error_response(json!({
            "error": "package_install_failed",
            "code": failure.code,
            "category": failure.category,
            "message": failure.message,
        }));
    }
}
