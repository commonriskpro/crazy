// ── ail-package::versioning ───────────────────────────────────────────────
//
// Versioning metadata, compatibility rules, and migration records for AIL
// packages.
//
// # Design (docs/packages.md §Versioning)
//
// Packages carry semantic versioning plus schema compatibility metadata:
//   package_version  1.2.0
//   graph_schema     3
//   core_ir_schema   2
//   acl_version      1.0
//
// Compatibility rules:
//   patch  — no public contract/effect changes
//   minor  — additive compatible exports
//   major  — breaking signatures/contracts/effects
//
// Breaking changes require migration metadata:
//   migration payments.stripe 1.0.0 -> 2.0.0
//     changed capability payment.charge
//     replacement payment.authorize + payment.capture
//   end

use blake3::Hasher;
use semver::Version;
use serde::{Deserialize, Serialize};

// ── CompatibilityClass ────────────────────────────────────────────────────

/// Classification of a version change relative to the declared compatibility
/// rules.
///
/// Maps directly to SemVer bump semantics in the AIL package model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompatibilityClass {
    /// Patch bump — no public contract or effect changes permitted.
    Patch,
    /// Minor bump — additive compatible exports only; existing APIs unchanged.
    Minor,
    /// Major bump — breaking changes to signatures, contracts, or effects.
    Major,
}

impl std::fmt::Display for CompatibilityClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompatibilityClass::Patch => write!(f, "patch"),
            CompatibilityClass::Minor => write!(f, "minor"),
            CompatibilityClass::Major => write!(f, "major"),
        }
    }
}

// ── MigrationStep ─────────────────────────────────────────────────────────

/// One changed item and its replacement in a migration record.
///
/// Records what changed (e.g., a capability ID) and what the replacement is
/// (e.g., a comma-separated list of replacement capability IDs).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationStep {
    /// The item that changed (e.g., a capability ID `"payment.charge"`).
    pub changed: String,
    /// Replacement item(s) (e.g., `"payment.authorize + payment.capture"`).
    pub replacement: String,
}

// ── MigrationRecord ───────────────────────────────────────────────────────

/// Migration metadata for a breaking version change.
///
/// Required for major version bumps where existing callers must update their
/// imports, grants, or handler bindings.
///
/// # Example (from docs/packages.md)
/// ```text
/// migration payments.stripe 1.0.0 -> 2.0.0
///   changed capability payment.charge
///   replacement payment.authorize + payment.capture
/// end
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationRecord {
    /// Name of the package this migration applies to.
    pub package: String,
    /// Source version this migration starts from (e.g., `"1.0.0"`).
    ///
    /// Local enforcement normalizes SemVer shorthand such as `"1"` or `"1.0"`,
    /// but does not interpret ranges or wildcards.
    pub from_version: String,
    /// Target version this migration leads to (e.g., `"2.0.0"`).
    pub to_version: String,
    /// Ordered list of changed items and their replacements.
    pub steps: Vec<MigrationStep>,
}

impl MigrationRecord {
    /// Compute a deterministic BLAKE3 hash of this migration metadata.
    ///
    /// The hash identifies the local migration record that justified accepting
    /// a breaking package upgrade. It does not execute the migration.
    pub fn blake3_hex(&self) -> Result<String, String> {
        let mut buf = Vec::new();
        ciborium::ser::into_writer(self, &mut buf)
            .map_err(|e| format!("CBOR serialization failed: {e}"))?;
        let mut hasher = Hasher::new();
        hasher.update(&buf);
        Ok(hasher.finalize().to_hex().to_string())
    }
}

// ── PackageCompatibilityMetadata ──────────────────────────────────────────

/// Local compatibility metadata for one package version.
///
/// This is intentionally local registry metadata. It records the compatibility
/// class for a package release plus any migration metadata required to justify
/// accepting a breaking upgrade. It is not a remote registry protocol and does
/// not execute migrations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageCompatibilityMetadata {
    /// Package this metadata applies to.
    pub package: String,
    /// Package version this metadata applies to.
    pub version: String,
    /// Declared compatibility class for this package version.
    pub compatibility: CompatibilityClass,
    /// Migration records for breaking upgrades into this version.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub migrations: Vec<MigrationRecord>,
}

// ── LocalCompatibilityIssue ───────────────────────────────────────────────

/// Stable issue kind for local compatibility/migration checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalCompatibilityIssueKind {
    Compatibility,
    Migration,
}

impl std::fmt::Display for LocalCompatibilityIssueKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LocalCompatibilityIssueKind::Compatibility => write!(f, "compatibility"),
            LocalCompatibilityIssueKind::Migration => write!(f, "migration"),
        }
    }
}

/// Local compatibility or migration issue discovered for a package upgrade.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalCompatibilityIssue {
    pub package: String,
    pub current_version: String,
    pub target_version: String,
    pub kind: LocalCompatibilityIssueKind,
    pub reason: String,
    pub migration_hash: Option<String>,
}

// ── PackageVersioning ─────────────────────────────────────────────────────

/// Full versioning metadata for a package release.
///
/// Combines the SemVer string, schema compatibility versions, ACL version,
/// the classified compatibility class of this release relative to the
/// previous, and any migration records required for major bumps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageVersioning {
    /// SemVer package version string (e.g., `"1.2.0"`).
    pub version: String,
    /// Graph schema version this package was compiled against.
    pub graph_schema: u32,
    /// Core IR schema version this package was compiled against.
    pub core_ir_schema: u32,
    /// ACL (access-control layer) schema version (e.g., `"1.0"`).
    pub acl_version: String,
    /// Compatibility classification of this release.
    pub compatibility: CompatibilityClass,
    /// Migration records for breaking changes (required when `compatibility == Major`).
    ///
    /// Empty for patch/minor bumps.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub migrations: Vec<MigrationRecord>,
}

// ── CompatibilityEngine ───────────────────────────────────────────────────

/// Evaluates compatibility between two `PackageVersioning` records and
/// enforces the migration-metadata requirement for major bumps.
pub struct CompatibilityEngine;

/// Error returned by [`CompatibilityEngine::evaluate`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompatibilityError {
    /// A major bump was declared but no migration records were provided.
    MajorBumpWithoutMigration,
    /// A patch bump was declared with migration records (which imply breaking changes).
    PatchWithMigration,
    /// A version string could not be parsed as a semantic version.
    InvalidVersion(String),
    /// Compatibility metadata was for a different package or version.
    MetadataTargetMismatch,
    /// Migration metadata was for a different package.
    MigrationPackageMismatch,
    /// Migration metadata pointed at a different target version.
    MigrationTargetMismatch,
    /// Migration metadata could not be hashed deterministically.
    MigrationHashFailed(String),
}

impl std::fmt::Display for CompatibilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompatibilityError::MajorBumpWithoutMigration => {
                write!(
                    f,
                    "major version bump declared without required migration metadata"
                )
            }
            CompatibilityError::PatchWithMigration => {
                write!(
                    f,
                    "patch version bump must not carry migration records (implies breaking change)"
                )
            }
            CompatibilityError::InvalidVersion(version) => {
                write!(f, "invalid semantic version: {version}")
            }
            CompatibilityError::MetadataTargetMismatch => {
                write!(f, "compatibility metadata package/version mismatch")
            }
            CompatibilityError::MigrationPackageMismatch => {
                write!(f, "migration metadata package mismatch")
            }
            CompatibilityError::MigrationTargetMismatch => {
                write!(f, "migration metadata target version mismatch")
            }
            CompatibilityError::MigrationHashFailed(error) => {
                write!(f, "migration metadata hash failed: {error}")
            }
        }
    }
}

impl std::error::Error for CompatibilityError {}

impl CompatibilityEngine {
    /// Validate that a `PackageVersioning` record is internally consistent.
    ///
    /// Rules enforced:
    /// - `Major` compatibility MUST have at least one migration record.
    /// - `Patch` compatibility MUST NOT have any migration records.
    ///
    /// # Errors
    ///
    /// Returns the first `CompatibilityError` encountered.
    pub fn evaluate(versioning: &PackageVersioning) -> Result<(), CompatibilityError> {
        match versioning.compatibility {
            CompatibilityClass::Major if versioning.migrations.is_empty() => {
                Err(CompatibilityError::MajorBumpWithoutMigration)
            }
            CompatibilityClass::Patch if !versioning.migrations.is_empty() => {
                Err(CompatibilityError::PatchWithMigration)
            }
            _ => Ok(()),
        }
    }

    /// Validate local compatibility metadata for one package version.
    pub fn evaluate_local_metadata(
        metadata: &PackageCompatibilityMetadata,
    ) -> Result<(), CompatibilityError> {
        match metadata.compatibility {
            CompatibilityClass::Major if metadata.migrations.is_empty() => {
                return Err(CompatibilityError::MajorBumpWithoutMigration);
            }
            CompatibilityClass::Patch if !metadata.migrations.is_empty() => {
                return Err(CompatibilityError::PatchWithMigration);
            }
            _ => {}
        }

        for migration in &metadata.migrations {
            if migration.package != metadata.package {
                return Err(CompatibilityError::MigrationPackageMismatch);
            }
            if normalize_version(&migration.to_version)? != normalize_version(&metadata.version)? {
                return Err(CompatibilityError::MigrationTargetMismatch);
            }
        }

        Ok(())
    }

    /// Classify a version change using SemVer major/minor/patch semantics.
    pub fn classify_version_change(
        current_version: &str,
        target_version: &str,
    ) -> Result<CompatibilityClass, CompatibilityError> {
        let current = parse_version(current_version)?;
        let target = parse_version(target_version)?;
        if target.major != current.major {
            Ok(CompatibilityClass::Major)
        } else if target.minor != current.minor {
            Ok(CompatibilityClass::Minor)
        } else {
            Ok(CompatibilityClass::Patch)
        }
    }

    /// Evaluate whether a local upgrade has enough compatibility/migration metadata.
    ///
    /// Missing metadata for compatible patch/minor upgrades is accepted. Missing
    /// or invalid metadata for breaking upgrades returns blocked local issues.
    pub fn evaluate_local_upgrade(
        package: &str,
        current_version: &str,
        target_version: &str,
        target_metadata: Option<&PackageCompatibilityMetadata>,
    ) -> Result<Vec<LocalCompatibilityIssue>, CompatibilityError> {
        let inferred = Self::classify_version_change(current_version, target_version)?;
        let declared = target_metadata.map(|metadata| metadata.compatibility);
        let breaking =
            inferred == CompatibilityClass::Major || declared == Some(CompatibilityClass::Major);

        let Some(metadata) = target_metadata else {
            return if breaking {
                Ok(vec![LocalCompatibilityIssue {
                    package: package.to_string(),
                    current_version: current_version.to_string(),
                    target_version: target_version.to_string(),
                    kind: LocalCompatibilityIssueKind::Migration,
                    reason: "breaking upgrade requires local migration metadata".to_string(),
                    migration_hash: None,
                }])
            } else {
                Ok(Vec::new())
            };
        };

        if metadata.package != package
            || normalize_version(&metadata.version)? != normalize_version(target_version)?
        {
            return Err(CompatibilityError::MetadataTargetMismatch);
        }

        Self::evaluate_local_metadata(metadata)?;

        if breaking {
            let normalized_current_version = normalize_version(current_version)?;
            let normalized_target_version = normalize_version(target_version)?;
            let migration = metadata.migrations.iter().find(|migration| {
                migration.package == package
                    && normalize_version(&migration.from_version).ok().as_deref()
                        == Some(normalized_current_version.as_str())
                    && normalize_version(&migration.to_version).ok().as_deref()
                        == Some(normalized_target_version.as_str())
            });
            if let Some(migration) = migration {
                return Ok(vec![LocalCompatibilityIssue {
                    package: package.to_string(),
                    current_version: current_version.to_string(),
                    target_version: target_version.to_string(),
                    kind: LocalCompatibilityIssueKind::Migration,
                    reason: "breaking upgrade has local migration metadata".to_string(),
                    migration_hash: Some(
                        migration
                            .blake3_hex()
                            .map_err(CompatibilityError::MigrationHashFailed)?,
                    ),
                }]);
            }
            return Ok(vec![LocalCompatibilityIssue {
                package: package.to_string(),
                current_version: current_version.to_string(),
                target_version: target_version.to_string(),
                kind: LocalCompatibilityIssueKind::Migration,
                reason: "breaking upgrade requires local migration metadata".to_string(),
                migration_hash: None,
            }]);
        }

        Ok(Vec::new())
    }
}

fn parse_version(version: &str) -> Result<Version, CompatibilityError> {
    let normalized = normalize_version(version)?;
    Version::parse(&normalized).map_err(|_| CompatibilityError::InvalidVersion(version.to_string()))
}

fn normalize_version(version: &str) -> Result<String, CompatibilityError> {
    if Version::parse(version).is_ok() {
        return Ok(version.to_string());
    }

    let parts = version.split('.').collect::<Vec<_>>();
    match parts.as_slice() {
        [major, minor] if numeric_part(major) && numeric_part(minor) => {
            Ok(format!("{major}.{minor}.0"))
        }
        [major] if numeric_part(major) => Ok(format!("{major}.0.0")),
        _ => Err(CompatibilityError::InvalidVersion(version.to_string())),
    }
}

fn numeric_part(part: &str) -> bool {
    !part.is_empty() && part.chars().all(|c| c.is_ascii_digit())
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_migration() -> MigrationRecord {
        MigrationRecord {
            package: "payments.stripe".to_string(),
            from_version: "1.2.0".to_string(),
            to_version: "2.0.0".to_string(),
            steps: vec![MigrationStep {
                changed: "payment.charge".to_string(),
                replacement: "payment.authorize + payment.capture".to_string(),
            }],
        }
    }

    fn sample_versioning(compatibility: CompatibilityClass) -> PackageVersioning {
        PackageVersioning {
            version: "2.0.0".to_string(),
            graph_schema: 3,
            core_ir_schema: 2,
            acl_version: "1.0".to_string(),
            compatibility,
            migrations: vec![],
        }
    }

    fn sample_metadata(compatibility: CompatibilityClass) -> PackageCompatibilityMetadata {
        PackageCompatibilityMetadata {
            package: "payments.stripe".to_string(),
            version: "2.0.0".to_string(),
            compatibility,
            migrations: vec![],
        }
    }

    // ── versioning_cbor_round_trip ────────────────────────────────────────
    // Spec scenario: "PackageVersioning round-trips through CBOR"
    //   GIVEN a PackageVersioning with all fields set
    //   WHEN serialized and deserialized
    //   THEN all fields are equal to the original
    #[test]
    fn versioning_cbor_round_trip() {
        let mut v = sample_versioning(CompatibilityClass::Major);
        v.migrations.push(sample_migration());

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&v, &mut buf).expect("encode");
        let decoded: PackageVersioning = ciborium::de::from_reader(buf.as_slice()).expect("decode");

        assert_eq!(decoded, v);
    }

    // ── migration_record_cbor_round_trip ──────────────────────────────────
    // Spec scenario: "MigrationRecord round-trips through CBOR"
    #[test]
    fn migration_record_cbor_round_trip() {
        let m = sample_migration();
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&m, &mut buf).expect("encode");
        let decoded: MigrationRecord = ciborium::de::from_reader(buf.as_slice()).expect("decode");
        assert_eq!(decoded, m);
    }

    // ── compatibility_class_display ───────────────────────────────────────
    #[test]
    fn compatibility_class_display() {
        assert_eq!(CompatibilityClass::Patch.to_string(), "patch");
        assert_eq!(CompatibilityClass::Minor.to_string(), "minor");
        assert_eq!(CompatibilityClass::Major.to_string(), "major");
    }

    // ── major_bump_requires_migration ─────────────────────────────────────
    // Spec scenario: "Major version bump without migration is rejected"
    //   GIVEN a PackageVersioning with compatibility: Major and empty migrations
    //   WHEN evaluate() is called
    //   THEN it returns Err(MajorBumpWithoutMigration)
    #[test]
    fn major_bump_requires_migration() {
        let v = sample_versioning(CompatibilityClass::Major);
        assert_eq!(
            CompatibilityEngine::evaluate(&v),
            Err(CompatibilityError::MajorBumpWithoutMigration)
        );
    }

    // ── major_bump_with_migration_is_valid ────────────────────────────────
    // TRIANGULATE: Major with migration passes evaluation.
    #[test]
    fn major_bump_with_migration_is_valid() {
        let mut v = sample_versioning(CompatibilityClass::Major);
        v.migrations.push(sample_migration());
        assert_eq!(CompatibilityEngine::evaluate(&v), Ok(()));
    }

    // ── patch_bump_with_migration_is_rejected ─────────────────────────────
    // Spec scenario: "Patch bump with migration metadata is rejected"
    //   GIVEN a PackageVersioning with compatibility: Patch and a migration
    //   WHEN evaluate() is called
    //   THEN it returns Err(PatchWithMigration)
    #[test]
    fn patch_bump_with_migration_is_rejected() {
        let mut v = sample_versioning(CompatibilityClass::Patch);
        v.migrations.push(sample_migration());
        assert_eq!(
            CompatibilityEngine::evaluate(&v),
            Err(CompatibilityError::PatchWithMigration)
        );
    }

    // ── minor_bump_is_always_valid ────────────────────────────────────────
    // TRIANGULATE: Minor bump with or without migrations passes evaluation.
    #[test]
    fn minor_bump_is_valid_with_or_without_migrations() {
        let v = sample_versioning(CompatibilityClass::Minor);
        assert_eq!(CompatibilityEngine::evaluate(&v), Ok(()));
    }

    // ── patch_bump_without_migration_is_valid ─────────────────────────────
    // TRIANGULATE: Patch with no migrations passes evaluation.
    #[test]
    fn patch_bump_without_migration_is_valid() {
        let v = sample_versioning(CompatibilityClass::Patch);
        assert_eq!(CompatibilityEngine::evaluate(&v), Ok(()));
    }

    // ── acl_version_survives_round_trip ───────────────────────────────────
    // Spec scenario: "acl_version field round-trips through CBOR"
    #[test]
    fn acl_version_survives_round_trip() {
        let v = PackageVersioning {
            version: "1.2.0".to_string(),
            graph_schema: 3,
            core_ir_schema: 2,
            acl_version: "1.0".to_string(),
            compatibility: CompatibilityClass::Patch,
            migrations: vec![],
        };
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&v, &mut buf).expect("encode");
        let decoded: PackageVersioning = ciborium::de::from_reader(buf.as_slice()).expect("decode");
        assert_eq!(decoded.acl_version, "1.0");
    }

    #[test]
    fn local_compatible_upgrade_without_metadata_is_valid() {
        let issues =
            CompatibilityEngine::evaluate_local_upgrade("payments.stripe", "1.2.0", "1.3.0", None)
                .expect("compatible upgrade must evaluate");

        assert!(issues.is_empty());
    }

    #[test]
    fn local_breaking_upgrade_without_migration_is_reported() {
        let issues =
            CompatibilityEngine::evaluate_local_upgrade("payments.stripe", "1.2.0", "2.0.0", None)
                .expect("breaking upgrade must evaluate");

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, LocalCompatibilityIssueKind::Migration);
        assert_eq!(issues[0].migration_hash, None);
    }

    #[test]
    fn local_breaking_upgrade_with_migration_is_explicit() {
        let mut metadata = sample_metadata(CompatibilityClass::Major);
        metadata.migrations.push(sample_migration());

        let issues = CompatibilityEngine::evaluate_local_upgrade(
            "payments.stripe",
            "1.2",
            "2.0",
            Some(&metadata),
        )
        .expect("migration metadata must evaluate");

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, LocalCompatibilityIssueKind::Migration);
        assert!(issues[0].migration_hash.is_some());
    }

    #[test]
    fn local_breaking_upgrade_rejects_unrelated_migration_source() {
        let mut metadata = sample_metadata(CompatibilityClass::Major);
        metadata.migrations.push(MigrationRecord {
            package: "payments.stripe".to_string(),
            from_version: "999.0.0".to_string(),
            to_version: "2.0.0".to_string(),
            steps: vec![],
        });

        let issues = CompatibilityEngine::evaluate_local_upgrade(
            "payments.stripe",
            "1.2.0",
            "2.0.0",
            Some(&metadata),
        )
        .expect("migration metadata must evaluate");

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, LocalCompatibilityIssueKind::Migration);
        assert_eq!(issues[0].migration_hash, None);
    }

    #[test]
    fn local_major_metadata_requires_migration() {
        let metadata = sample_metadata(CompatibilityClass::Major);

        assert_eq!(
            CompatibilityEngine::evaluate_local_metadata(&metadata),
            Err(CompatibilityError::MajorBumpWithoutMigration)
        );
    }

    #[test]
    fn migration_hash_is_stable_lowercase_hex() {
        let hash = sample_migration().blake3_hex().expect("hash must compute");

        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(hash, hash.to_ascii_lowercase());
    }
}
