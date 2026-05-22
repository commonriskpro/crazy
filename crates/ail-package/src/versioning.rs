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
//   migration payments.stripe 1.x -> 2.0
//     changed capability payment.charge
//     replacement payment.authorize + payment.capture
//   end

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
/// migration payments.stripe 1.x -> 2.0
///   changed capability payment.charge
///   replacement payment.authorize + payment.capture
/// end
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationRecord {
    /// Name of the package this migration applies to.
    pub package: String,
    /// Human-readable from-version expression (e.g., `"1.x"`, `"<2.0"`).
    pub from_version: String,
    /// Target version this migration leads to (e.g., `"2.0.0"`).
    pub to_version: String,
    /// Ordered list of changed items and their replacements.
    pub steps: Vec<MigrationStep>,
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
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_migration() -> MigrationRecord {
        MigrationRecord {
            package: "payments.stripe".to_string(),
            from_version: "1.x".to_string(),
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
}
