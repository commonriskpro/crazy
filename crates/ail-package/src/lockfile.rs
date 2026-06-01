// ── ail-package::lockfile ─────────────────────────────────────────────────
//
// `LockfileEntry` and `Lockfile` — full lockfile workflow for reproducible
// package resolution.
//
// # Design (docs/packages.md §Reproducibility)
//
// Lockfile records:
//   name
//   version
//   requested_version
//   package_hash
//   trust_level
//   verification_report_hash
//   artifact_hashes
//   accepted_assumptions
//
// A `Lockfile` is an ordered collection of `LockfileEntry` records that
// pins an exact resolved dependency graph.  It can be generated from a
// resolver run and used to reproduce the same resolution deterministically.
//
// # Determinism contract
//
// All fields use deterministic types (String, Vec<String>, Option<String>).
// CBOR serialization via `ciborium` is byte-deterministic for this layout.

use std::collections::{BTreeMap, BTreeSet};

use blake3::Hasher;
use ciborium::ser::into_writer;
use serde::{Deserialize, Serialize};

use crate::manifest::{ArtifactHashEntry, PackageManifest};
use crate::resolver::DependencySpec;
use crate::trust::TrustLevel;

// ── LockfileEntry ─────────────────────────────────────────────────────────

/// One resolved and pinned package in the workspace lock file.
///
/// A `LockfileEntry` records the exact version and content hash of a
/// resolved package, the trust level at lock time, an optional link to
/// the verification report that produced this lock entry, and the set of
/// assumption IDs that were accepted by the approver.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockfileEntry {
    /// Package name (e.g., `"payments.stripe"`).
    pub name: String,
    /// Pinned semantic version string (e.g., `"2.3.1"`).
    pub version: String,
    /// Version requirement requested by the user or manifest before resolution.
    ///
    /// This is metadata only: `version` remains the exact resolved pin used for
    /// reproducible replay. Legacy lockfiles decode this as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_version: Option<String>,
    /// BLAKE3 hex digest of the package artifact at lock time.
    pub package_hash: String,
    /// Trust level recorded at lock time.
    pub trust_level: TrustLevel,
    /// Optional BLAKE3 hex digest of the verification report used to
    /// produce this lock entry.
    pub verification_report_hash: Option<String>,
    /// Artifact evidence copied from the package manifest at lock time.
    ///
    /// This includes executable artifacts such as `wasm-artifact` and their
    /// companion contract artifacts such as `wasm-abi-descriptor`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_hashes: Vec<ArtifactHashEntry>,
    /// Assumption IDs accepted by the approver at lock time, in canonical lexical order.
    ///
    /// Uses `Vec` (not `HashSet`) to maintain CBOR determinism.
    pub accepted_assumptions: Vec<String>,
}

/// Stable high-level category for lockfile validation issues.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LockfileValidationCategory {
    /// Canonical ordering or uniqueness needed for deterministic replay.
    Determinism,
    /// Invalid or incomplete data stored in the lockfile itself.
    LockfileIntegrity,
    /// Drift between the lockfile and actual replay artifacts.
    ReplayIntegrity,
}

impl std::fmt::Display for LockfileValidationCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockfileValidationCategory::Determinism => write!(f, "determinism"),
            LockfileValidationCategory::LockfileIntegrity => write!(f, "lockfile_integrity"),
            LockfileValidationCategory::ReplayIntegrity => write!(f, "replay_integrity"),
        }
    }
}

/// Stable machine-readable issue kind emitted by lockfile validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LockfileValidationIssueKind {
    /// Entries are not in canonical `(name, version)` order.
    UnstableEntryOrder,
    /// The same `(name, version)` package is pinned more than once.
    DuplicatePackageEntry,
    /// The caller supplied the same actual package coordinate more than once.
    DuplicateActualPackage,
    /// A locked package was not present in the actual package set.
    MissingPackage,
    /// A locked package exists, but its artifact digest differs from the lock.
    PackageHashMismatch,
    /// Locked artifact evidence differs from replay metadata.
    ArtifactHashMismatch,
    /// A locked package has no package artifact digest.
    EmptyPackageHash,
    /// A locked package records an empty verification report digest.
    EmptyVerificationReportHash,
    /// A locked WASM artifact is missing its ABI descriptor artifact evidence.
    MissingAbiDescriptorArtifact,
    /// A locked package records an empty accepted assumption ID.
    EmptyAcceptedAssumption,
    /// A locked package records the same accepted assumption more than once.
    DuplicateAcceptedAssumption,
    /// Accepted assumptions are not in canonical lexical order.
    UnstableAcceptedAssumptionOrder,
}

impl LockfileValidationIssueKind {
    /// Stable issue code for downstream tooling and reports.
    pub fn code(self) -> &'static str {
        match self {
            LockfileValidationIssueKind::UnstableEntryOrder => "LOCKFILE_UNSTABLE_ENTRY_ORDER",
            LockfileValidationIssueKind::DuplicatePackageEntry => {
                "LOCKFILE_DUPLICATE_PACKAGE_ENTRY"
            }
            LockfileValidationIssueKind::DuplicateActualPackage => {
                "LOCKFILE_DUPLICATE_ACTUAL_PACKAGE"
            }
            LockfileValidationIssueKind::MissingPackage => "LOCKFILE_MISSING_PACKAGE",
            LockfileValidationIssueKind::PackageHashMismatch => "LOCKFILE_PACKAGE_HASH_MISMATCH",
            LockfileValidationIssueKind::ArtifactHashMismatch => "LOCKFILE_ARTIFACT_HASH_MISMATCH",
            LockfileValidationIssueKind::EmptyPackageHash => "LOCKFILE_EMPTY_PACKAGE_HASH",
            LockfileValidationIssueKind::EmptyVerificationReportHash => {
                "LOCKFILE_EMPTY_VERIFICATION_REPORT_HASH"
            }
            LockfileValidationIssueKind::MissingAbiDescriptorArtifact => {
                "LOCKFILE_MISSING_ABI_DESCRIPTOR_ARTIFACT"
            }
            LockfileValidationIssueKind::EmptyAcceptedAssumption => {
                "LOCKFILE_EMPTY_ACCEPTED_ASSUMPTION"
            }
            LockfileValidationIssueKind::DuplicateAcceptedAssumption => {
                "LOCKFILE_DUPLICATE_ACCEPTED_ASSUMPTION"
            }
            LockfileValidationIssueKind::UnstableAcceptedAssumptionOrder => {
                "LOCKFILE_UNSTABLE_ACCEPTED_ASSUMPTION_ORDER"
            }
        }
    }

    /// Stable category for issue aggregation.
    pub fn category(self) -> LockfileValidationCategory {
        match self {
            LockfileValidationIssueKind::UnstableEntryOrder
            | LockfileValidationIssueKind::DuplicatePackageEntry
            | LockfileValidationIssueKind::DuplicateActualPackage
            | LockfileValidationIssueKind::DuplicateAcceptedAssumption
            | LockfileValidationIssueKind::UnstableAcceptedAssumptionOrder => {
                LockfileValidationCategory::Determinism
            }
            LockfileValidationIssueKind::EmptyPackageHash
            | LockfileValidationIssueKind::EmptyVerificationReportHash
            | LockfileValidationIssueKind::MissingAbiDescriptorArtifact
            | LockfileValidationIssueKind::EmptyAcceptedAssumption => {
                LockfileValidationCategory::LockfileIntegrity
            }
            LockfileValidationIssueKind::MissingPackage
            | LockfileValidationIssueKind::PackageHashMismatch
            | LockfileValidationIssueKind::ArtifactHashMismatch => {
                LockfileValidationCategory::ReplayIntegrity
            }
        }
    }
}

impl std::fmt::Display for LockfileValidationIssueKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code())
    }
}

/// Reproducibility and integrity problems found in a lockfile validation pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LockfileValidationIssue {
    /// Entries are not in canonical `(name, version)` order.
    UnstableEntryOrder {
        previous_name: String,
        previous_version: String,
        name: String,
        version: String,
    },
    /// The same `(name, version)` package is pinned more than once.
    DuplicatePackageEntry { name: String, version: String },
    /// The caller supplied the same actual package coordinate more than once.
    DuplicateActualPackage { name: String, version: String },
    /// A locked package was not present in the actual package set.
    MissingPackage { name: String, version: String },
    /// A locked package exists, but its artifact digest differs from the lock.
    PackageHashMismatch {
        name: String,
        version: String,
        expected: String,
        actual: String,
    },
    /// Locked artifact evidence differs from replay metadata.
    ArtifactHashMismatch {
        name: String,
        version: String,
        expected: String,
        actual: String,
    },
    /// A locked package has no package artifact digest.
    EmptyPackageHash { name: String, version: String },
    /// A locked package records an empty verification report digest.
    EmptyVerificationReportHash { name: String, version: String },
    /// A locked WASM artifact is missing its ABI descriptor artifact evidence.
    MissingAbiDescriptorArtifact { name: String, version: String },
    /// A locked package records an empty accepted assumption ID.
    EmptyAcceptedAssumption { name: String, version: String },
    /// A locked package records the same accepted assumption more than once.
    DuplicateAcceptedAssumption {
        name: String,
        version: String,
        assumption: String,
    },
    /// Accepted assumptions are not in canonical lexical order.
    UnstableAcceptedAssumptionOrder {
        name: String,
        version: String,
        previous: String,
        assumption: String,
    },
}

impl LockfileValidationIssue {
    /// Stable machine-readable kind for this issue.
    pub fn kind(&self) -> LockfileValidationIssueKind {
        match self {
            LockfileValidationIssue::UnstableEntryOrder { .. } => {
                LockfileValidationIssueKind::UnstableEntryOrder
            }
            LockfileValidationIssue::DuplicatePackageEntry { .. } => {
                LockfileValidationIssueKind::DuplicatePackageEntry
            }
            LockfileValidationIssue::DuplicateActualPackage { .. } => {
                LockfileValidationIssueKind::DuplicateActualPackage
            }
            LockfileValidationIssue::MissingPackage { .. } => {
                LockfileValidationIssueKind::MissingPackage
            }
            LockfileValidationIssue::PackageHashMismatch { .. } => {
                LockfileValidationIssueKind::PackageHashMismatch
            }
            LockfileValidationIssue::ArtifactHashMismatch { .. } => {
                LockfileValidationIssueKind::ArtifactHashMismatch
            }
            LockfileValidationIssue::EmptyPackageHash { .. } => {
                LockfileValidationIssueKind::EmptyPackageHash
            }
            LockfileValidationIssue::EmptyVerificationReportHash { .. } => {
                LockfileValidationIssueKind::EmptyVerificationReportHash
            }
            LockfileValidationIssue::MissingAbiDescriptorArtifact { .. } => {
                LockfileValidationIssueKind::MissingAbiDescriptorArtifact
            }
            LockfileValidationIssue::EmptyAcceptedAssumption { .. } => {
                LockfileValidationIssueKind::EmptyAcceptedAssumption
            }
            LockfileValidationIssue::DuplicateAcceptedAssumption { .. } => {
                LockfileValidationIssueKind::DuplicateAcceptedAssumption
            }
            LockfileValidationIssue::UnstableAcceptedAssumptionOrder { .. } => {
                LockfileValidationIssueKind::UnstableAcceptedAssumptionOrder
            }
        }
    }

    /// Stable issue code for downstream tooling and reports.
    pub fn code(&self) -> &'static str {
        self.kind().code()
    }

    /// Stable high-level category for issue aggregation.
    pub fn category(&self) -> LockfileValidationCategory {
        self.kind().category()
    }
}

/// Actual artifact evidence observed for one resolved package during replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockfileArtifactEvidence {
    pub name: String,
    pub version: String,
    pub artifact_hashes: Vec<ArtifactHashEntry>,
}

impl LockfileArtifactEvidence {
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        artifact_hashes: Vec<ArtifactHashEntry>,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            artifact_hashes,
        }
    }
}

/// Stable machine-readable issue kind emitted by production lockfile diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LockfileIntegrityIssueKind {
    /// Entries are not in canonical `(name, version)` order.
    UnstableEntryOrder,
    /// The same `(name, version)` package is pinned more than once.
    DuplicateLockfilePackage,
    /// Replay metadata supplied the same `(name, version)` more than once.
    DuplicateResolvedPackage,
    /// A locked package was not present in the replay metadata.
    MissingPackage,
    /// A locked package exists, but its artifact digest differs from replay.
    PackageHashMismatch,
    /// Locked artifact evidence differs from replay metadata.
    ArtifactHashMismatch,
    /// A locked package's source digest differs from replay metadata.
    SourceHashMismatch,
    /// Replay metadata did not include a resolved source descriptor.
    MissingResolvedSource,
    /// Replay metadata reports a graph schema below the required floor.
    StaleGraphSchemaVersion,
    /// Replay metadata reports a Core IR schema below the required floor.
    StaleCoreIrSchemaVersion,
    /// A locked package has no package artifact digest.
    EmptyPackageHash,
    /// A locked package records an empty verification report digest.
    EmptyVerificationReportHash,
    /// A locked WASM artifact is missing its ABI descriptor artifact evidence.
    MissingAbiDescriptorArtifact,
    /// A locked package records an empty accepted assumption ID.
    EmptyAcceptedAssumption,
    /// A locked package records the same accepted assumption more than once.
    DuplicateAcceptedAssumption,
    /// Accepted assumptions are not in canonical lexical order.
    UnstableAcceptedAssumptionOrder,
}

impl LockfileIntegrityIssueKind {
    /// Stable issue code for downstream tooling and reports.
    pub fn code(self) -> &'static str {
        match self {
            LockfileIntegrityIssueKind::UnstableEntryOrder => "LOCKFILE_UNSTABLE_ENTRY_ORDER",
            LockfileIntegrityIssueKind::DuplicateLockfilePackage => {
                "LOCKFILE_DUPLICATE_LOCKFILE_PACKAGE"
            }
            LockfileIntegrityIssueKind::DuplicateResolvedPackage => {
                "LOCKFILE_DUPLICATE_RESOLVED_PACKAGE"
            }
            LockfileIntegrityIssueKind::MissingPackage => "LOCKFILE_MISSING_PACKAGE",
            LockfileIntegrityIssueKind::PackageHashMismatch => "LOCKFILE_PACKAGE_HASH_MISMATCH",
            LockfileIntegrityIssueKind::ArtifactHashMismatch => "LOCKFILE_ARTIFACT_HASH_MISMATCH",
            LockfileIntegrityIssueKind::SourceHashMismatch => "LOCKFILE_SOURCE_HASH_MISMATCH",
            LockfileIntegrityIssueKind::MissingResolvedSource => "LOCKFILE_MISSING_RESOLVED_SOURCE",
            LockfileIntegrityIssueKind::StaleGraphSchemaVersion => {
                "LOCKFILE_STALE_GRAPH_SCHEMA_VERSION"
            }
            LockfileIntegrityIssueKind::StaleCoreIrSchemaVersion => {
                "LOCKFILE_STALE_CORE_IR_SCHEMA_VERSION"
            }
            LockfileIntegrityIssueKind::EmptyPackageHash => "LOCKFILE_EMPTY_PACKAGE_HASH",
            LockfileIntegrityIssueKind::EmptyVerificationReportHash => {
                "LOCKFILE_EMPTY_VERIFICATION_REPORT_HASH"
            }
            LockfileIntegrityIssueKind::MissingAbiDescriptorArtifact => {
                "LOCKFILE_MISSING_ABI_DESCRIPTOR_ARTIFACT"
            }
            LockfileIntegrityIssueKind::EmptyAcceptedAssumption => {
                "LOCKFILE_EMPTY_ACCEPTED_ASSUMPTION"
            }
            LockfileIntegrityIssueKind::DuplicateAcceptedAssumption => {
                "LOCKFILE_DUPLICATE_ACCEPTED_ASSUMPTION"
            }
            LockfileIntegrityIssueKind::UnstableAcceptedAssumptionOrder => {
                "LOCKFILE_UNSTABLE_ACCEPTED_ASSUMPTION_ORDER"
            }
        }
    }

    /// Stable category for low-cardinality aggregation.
    pub fn category(self) -> LockfileValidationCategory {
        match self {
            LockfileIntegrityIssueKind::UnstableEntryOrder
            | LockfileIntegrityIssueKind::DuplicateLockfilePackage
            | LockfileIntegrityIssueKind::DuplicateResolvedPackage
            | LockfileIntegrityIssueKind::DuplicateAcceptedAssumption
            | LockfileIntegrityIssueKind::UnstableAcceptedAssumptionOrder => {
                LockfileValidationCategory::Determinism
            }
            LockfileIntegrityIssueKind::EmptyPackageHash
            | LockfileIntegrityIssueKind::EmptyVerificationReportHash
            | LockfileIntegrityIssueKind::MissingAbiDescriptorArtifact
            | LockfileIntegrityIssueKind::EmptyAcceptedAssumption
            | LockfileIntegrityIssueKind::MissingResolvedSource
            | LockfileIntegrityIssueKind::StaleGraphSchemaVersion
            | LockfileIntegrityIssueKind::StaleCoreIrSchemaVersion => {
                LockfileValidationCategory::LockfileIntegrity
            }
            LockfileIntegrityIssueKind::MissingPackage
            | LockfileIntegrityIssueKind::PackageHashMismatch
            | LockfileIntegrityIssueKind::ArtifactHashMismatch
            | LockfileIntegrityIssueKind::SourceHashMismatch => {
                LockfileValidationCategory::ReplayIntegrity
            }
        }
    }
}

impl std::fmt::Display for LockfileIntegrityIssueKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code())
    }
}

/// Redacted package descriptor emitted by lockfile integrity diagnostics.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockfilePackageDescriptor {
    /// Package name associated with the issue.
    pub name: String,
    /// Package version associated with the issue.
    pub version: String,
    /// Resolved source shape with credentials, path, query, and fragment removed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_source: Option<String>,
    /// Always true: diagnostics must not expose raw source descriptors.
    pub redacted: bool,
}

/// Redacted, stable issue emitted by production lockfile integrity diagnostics.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockfileIntegrityIssue {
    /// Stable machine-readable issue kind.
    pub kind: LockfileIntegrityIssueKind,
    /// Stable issue code for downstream tooling and reports.
    pub code: String,
    /// Stable category for low-cardinality aggregation.
    pub category: LockfileValidationCategory,
    /// Redacted package descriptor.
    pub package: LockfilePackageDescriptor,
    /// Expected digest/version/detail, when the issue has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    /// Actual digest/version/detail, when the issue has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
}

impl LockfileIntegrityIssue {
    fn new(kind: LockfileIntegrityIssueKind, name: impl ToString, version: impl ToString) -> Self {
        Self::with_source(kind, name, version, None)
    }

    fn with_source(
        kind: LockfileIntegrityIssueKind,
        name: impl ToString,
        version: impl ToString,
        resolved_source: Option<&str>,
    ) -> Self {
        Self {
            kind,
            code: kind.code().to_string(),
            category: kind.category(),
            package: LockfilePackageDescriptor {
                name: name.to_string(),
                version: version.to_string(),
                resolved_source: resolved_source.map(redacted_resolved_source),
                redacted: true,
            },
            expected: None,
            actual: None,
        }
    }

    fn with_expected_actual(mut self, expected: impl ToString, actual: impl ToString) -> Self {
        self.expected = Some(expected.to_string());
        self.actual = Some(actual.to_string());
        self
    }
}

/// Replay-time resolved package metadata for lockfile integrity diagnostics.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockfileResolvedPackage {
    /// Package name resolved during replay.
    pub name: String,
    /// Package version resolved during replay.
    pub version: String,
    /// Actual package artifact hash observed during replay.
    pub package_hash: String,
    /// Resolved source descriptor (registry URL, archive URI, or repository reference).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_source: Option<String>,
    /// Source digest expected by lock/replay metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_source_hash: Option<String>,
    /// Actual source digest observed during replay.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_source_hash: Option<String>,
    /// Graph schema version observed on the replayed package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_schema: Option<u32>,
    /// Core IR schema version observed on the replayed package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub core_ir_schema: Option<u32>,
}

impl LockfileResolvedPackage {
    /// Construct replay metadata with the fields needed by legacy hash validation.
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        package_hash: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            package_hash: package_hash.into(),
            resolved_source: None,
            expected_source_hash: None,
            actual_source_hash: None,
            graph_schema: None,
            core_ir_schema: None,
        }
    }

    /// Attach the resolved source descriptor observed during replay.
    pub fn with_resolved_source(mut self, source: impl Into<String>) -> Self {
        self.resolved_source = Some(source.into());
        self
    }

    /// Attach expected and actual source digests for source-integrity checks.
    pub fn with_source_hashes(
        mut self,
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        self.expected_source_hash = Some(expected.into());
        self.actual_source_hash = Some(actual.into());
        self
    }

    /// Attach package schema versions observed during replay.
    pub fn with_schema_versions(
        mut self,
        graph_schema: Option<u32>,
        core_ir_schema: Option<u32>,
    ) -> Self {
        self.graph_schema = graph_schema;
        self.core_ir_schema = core_ir_schema;
        self
    }
}

/// Additional production integrity gates for replay-time lockfile validation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockfileValidationRequirements {
    /// Require every replayed package to include a resolved source descriptor.
    pub require_resolved_source: bool,
    /// Minimum accepted graph schema version for replayed packages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_graph_schema: Option<u32>,
    /// Minimum accepted Core IR schema version for replayed packages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_core_ir_schema: Option<u32>,
}

// ── Lockfile ──────────────────────────────────────────────────────────────

/// A resolved and pinned dependency graph — the full lockfile.
///
/// A `Lockfile` is a canonically ordered collection of `LockfileEntry` records.
/// It is produced by the dependency resolver after a successful resolution
/// run and can be used to reproduce the same graph deterministically.
///
/// The lockfile itself is content-addressed via
/// [`Lockfile::blake3_hex`] — hashing the canonical CBOR encoding of all
/// entries in canonical `(name, version)` order.
///
/// See `docs/packages.md` §Reproducibility.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lockfile {
    /// Pinned package entries in canonical `(name, version)` order.
    pub entries: Vec<LockfileEntry>,
}

impl Lockfile {
    /// Create an empty lockfile.
    pub fn new() -> Self {
        Lockfile::default()
    }

    /// Add a resolved entry to the lockfile.
    ///
    /// This preserves caller order; use [`Lockfile::validate_reproducibility`]
    /// to reject non-canonical locks before publishing or replaying them.
    pub fn add(&mut self, entry: LockfileEntry) {
        self.entries.push(entry);
    }

    /// Return `true` if the lockfile contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Return the number of pinned packages.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Look up a pinned entry by package name and version.
    pub fn get(&self, name: &str, version: &str) -> Option<&LockfileEntry> {
        self.entries
            .iter()
            .find(|e| e.name == name && e.version == version)
    }

    /// Compute the BLAKE3 content hash of this lockfile as a hex-encoded string.
    ///
    /// The hash covers the canonical CBOR serialization of all entries in
    /// insertion order, providing a stable fingerprint of the full resolved graph.
    ///
    /// # Errors
    ///
    /// Returns `Err` if CBOR serialization fails.
    pub fn blake3_hex(&self) -> Result<String, String> {
        let mut buf = Vec::new();
        into_writer(self, &mut buf).map_err(|e| format!("CBOR serialization failed: {e}"))?;
        let mut hasher = Hasher::new();
        hasher.update(&buf);
        Ok(hasher.finalize().to_hex().to_string())
    }

    /// Build a `Lockfile` from a set of resolved `(DependencySpec, PackageManifest)` pairs.
    ///
    /// Each resolution is pinned to the manifest's exact version.  The
    /// `package_hash` is derived from `PackageManifest::blake3_hex()`; if
    /// hashing fails the field is set to an empty string. Entries are sorted
    /// by `(name, version)` so the resulting lock does not depend on resolver
    /// traversal order.
    pub fn from_resolution(resolutions: Vec<(&DependencySpec, &PackageManifest)>) -> Self {
        let mut entries: Vec<_> = resolutions
            .into_iter()
            .map(|(spec, manifest)| {
                let package_hash = manifest.blake3_hex().unwrap_or_default();
                let verification_report_hash = manifest
                    .verification_report
                    .as_ref()
                    .and_then(|report| report.blake3_hex().ok());
                LockfileEntry {
                    name: manifest.name.clone(),
                    version: manifest.version.clone(),
                    requested_version: Some(spec.version_constraint.clone()),
                    package_hash,
                    trust_level: manifest.trust_level,
                    verification_report_hash,
                    artifact_hashes: manifest.artifact_hashes.clone(),
                    accepted_assumptions: vec![],
                }
            })
            .collect();
        entries.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
        Lockfile { entries }
    }

    /// Convert all lockfile entries back to exact-version `DependencySpec`s.
    ///
    /// The returned specs use `version_constraint = entry.version` (exact pin)
    /// and `min_trust = TrustLevel::Unverified` (the caller can tighten this).
    pub fn to_specs(&self) -> Vec<DependencySpec> {
        self.entries
            .iter()
            .map(|entry| DependencySpec {
                name: entry.name.clone(),
                version_constraint: entry.version.clone(),
                min_trust: TrustLevel::Unverified,
                profile: None,
                allowed_licenses: vec![],
                denied_capabilities: vec![],
                denied_handlers: vec![],
                min_graph_schema: None,
                min_core_ir_schema: None,
            })
            .collect()
    }

    /// Validate that this lockfile is deterministic and matches the actual package set.
    ///
    /// The reproducibility contract is intentionally stricter than
    /// [`Lockfile::verify_integrity`]: entries must be in canonical
    /// `(name, version)` order, each `(name, version)` coordinate must appear
    /// exactly once, and every locked package must match the actual artifact
    /// digest supplied by the caller.
    pub fn validate_reproducibility(
        &self,
        actual: &[(&str, &str, &str)],
    ) -> Vec<LockfileValidationIssue> {
        let mut issues = Vec::new();
        let mut previous: Option<(&LockfileEntry, (&str, &str))> = None;
        let mut locked_seen = BTreeSet::new();

        for entry in &self.entries {
            let coordinate = (entry.name.as_str(), entry.version.as_str());
            if let Some((previous_entry, previous_coordinate)) = previous {
                if coordinate < previous_coordinate {
                    issues.push(LockfileValidationIssue::UnstableEntryOrder {
                        previous_name: previous_entry.name.clone(),
                        previous_version: previous_entry.version.clone(),
                        name: entry.name.clone(),
                        version: entry.version.clone(),
                    });
                }
            }

            if !locked_seen.insert((entry.name.clone(), entry.version.clone())) {
                issues.push(LockfileValidationIssue::DuplicatePackageEntry {
                    name: entry.name.clone(),
                    version: entry.version.clone(),
                });
            }

            if entry.package_hash.is_empty() {
                issues.push(LockfileValidationIssue::EmptyPackageHash {
                    name: entry.name.clone(),
                    version: entry.version.clone(),
                });
            }

            if matches!(entry.verification_report_hash.as_deref(), Some("")) {
                issues.push(LockfileValidationIssue::EmptyVerificationReportHash {
                    name: entry.name.clone(),
                    version: entry.version.clone(),
                });
            }

            validate_accepted_assumptions(entry, &mut issues);
            validate_artifact_evidence(entry, &mut issues);
            previous = Some((entry, coordinate));
        }

        let mut actual_hashes_by_coordinate: BTreeMap<(&str, &str), BTreeSet<&str>> =
            BTreeMap::new();
        let mut actual_counts_by_coordinate: BTreeMap<(&str, &str), usize> = BTreeMap::new();
        for (name, version, hash) in actual {
            actual_hashes_by_coordinate
                .entry((*name, *version))
                .or_default()
                .insert(*hash);
            *actual_counts_by_coordinate
                .entry((*name, *version))
                .or_default() += 1;
        }

        for ((name, version), count) in actual_counts_by_coordinate {
            if count > 1 {
                issues.push(LockfileValidationIssue::DuplicateActualPackage {
                    name: name.to_string(),
                    version: version.to_string(),
                });
            }
        }

        for entry in &self.entries {
            match actual_hashes_by_coordinate.get(&(entry.name.as_str(), entry.version.as_str())) {
                Some(actual_hashes) if actual_hashes.contains(entry.package_hash.as_str()) => {}
                Some(actual_hashes) => {
                    issues.push(LockfileValidationIssue::PackageHashMismatch {
                        name: entry.name.clone(),
                        version: entry.version.clone(),
                        expected: entry.package_hash.clone(),
                        actual: actual_hashes.iter().copied().collect::<Vec<_>>().join(","),
                    });
                }
                None => {
                    issues.push(LockfileValidationIssue::MissingPackage {
                        name: entry.name.clone(),
                        version: entry.version.clone(),
                    });
                }
            }
        }

        sort_validation_issues(&mut issues);
        issues
    }

    /// Validate that locked artifact evidence matches replay metadata.
    ///
    /// Package hashes prove the manifest bytes, but production package replay
    /// also needs the lockfile-visible artifact evidence to remain identical:
    /// tools should catch a hand-edited or legacy lockfile that no longer
    /// records the artifact roles/hashes present in the resolved manifest.
    pub fn validate_artifact_reproducibility(
        &self,
        actual: &[LockfileArtifactEvidence],
    ) -> Vec<LockfileValidationIssue> {
        let mut actual_by_coordinate: BTreeMap<(&str, &str), String> = BTreeMap::new();
        for package in actual {
            actual_by_coordinate.insert(
                (package.name.as_str(), package.version.as_str()),
                canonical_artifact_hashes(&package.artifact_hashes),
            );
        }

        let mut issues = Vec::new();
        for entry in &self.entries {
            let expected = canonical_artifact_hashes(&entry.artifact_hashes);
            let actual = actual_by_coordinate
                .get(&(entry.name.as_str(), entry.version.as_str()))
                .cloned()
                .unwrap_or_default();
            if expected != actual {
                issues.push(LockfileValidationIssue::ArtifactHashMismatch {
                    name: entry.name.clone(),
                    version: entry.version.clone(),
                    expected,
                    actual,
                });
            }
        }
        sort_validation_issues(&mut issues);
        issues
    }

    /// Produce stable, redacted production diagnostics for lockfile replay integrity.
    ///
    /// This keeps [`Lockfile::validate_reproducibility`] compatible while giving
    /// package-management workflows richer checks for source drift, missing
    /// resolved source descriptors, schema floors, and deterministic issue order.
    pub fn diagnose_integrity(
        &self,
        actual: &[LockfileResolvedPackage],
        requirements: &LockfileValidationRequirements,
    ) -> Vec<LockfileIntegrityIssue> {
        let actual_tuples = actual
            .iter()
            .map(|package| {
                (
                    package.name.as_str(),
                    package.version.as_str(),
                    package.package_hash.as_str(),
                )
            })
            .collect::<Vec<_>>();
        let mut issues = self
            .validate_reproducibility(&actual_tuples)
            .iter()
            .map(integrity_issue_from_validation_issue)
            .collect::<Vec<_>>();

        let mut actual_by_coordinate: BTreeMap<(String, String), ResolvedPackageFacts> =
            BTreeMap::new();
        for package in actual {
            actual_by_coordinate
                .entry((package.name.clone(), package.version.clone()))
                .or_default()
                .record(package);
        }

        for ((name, version), facts) in &actual_by_coordinate {
            if requirements.require_resolved_source && facts.has_missing_resolved_source {
                issues.push(LockfileIntegrityIssue::new(
                    LockfileIntegrityIssueKind::MissingResolvedSource,
                    name,
                    version,
                ));
            }

            if !facts.expected_source_hashes.is_empty()
                && facts.expected_source_hashes != facts.actual_source_hashes
            {
                issues.push(
                    LockfileIntegrityIssue::with_source(
                        LockfileIntegrityIssueKind::SourceHashMismatch,
                        name,
                        version,
                        facts.first_resolved_source().as_deref(),
                    )
                    .with_expected_actual(
                        join_strings(&facts.expected_source_hashes),
                        join_strings(&facts.actual_source_hashes),
                    ),
                );
            }

            if let Some(minimum) = requirements.min_graph_schema {
                let actual = facts.lowest_graph_schema();
                if actual.unwrap_or(0) < minimum {
                    issues.push(
                        LockfileIntegrityIssue::new(
                            LockfileIntegrityIssueKind::StaleGraphSchemaVersion,
                            name,
                            version,
                        )
                        .with_expected_actual(
                            minimum.to_string(),
                            actual.map(|value| value.to_string()).unwrap_or_default(),
                        ),
                    );
                }
            }

            if let Some(minimum) = requirements.min_core_ir_schema {
                let actual = facts.lowest_core_ir_schema();
                if actual.unwrap_or(0) < minimum {
                    issues.push(
                        LockfileIntegrityIssue::new(
                            LockfileIntegrityIssueKind::StaleCoreIrSchemaVersion,
                            name,
                            version,
                        )
                        .with_expected_actual(
                            minimum.to_string(),
                            actual.map(|value| value.to_string()).unwrap_or_default(),
                        ),
                    );
                }
            }
        }

        sort_integrity_issues(&mut issues);
        issues
    }

    /// Verify that all entries in this lockfile are present in the provided
    /// slice of `(name, version, hash)` tuples — confirming integrity.
    ///
    /// Returns the names of any entries whose `package_hash` does not match.
    pub fn verify_integrity<'a>(&'a self, actual: &[(&str, &str, &str)]) -> Vec<&'a str> {
        self.entries
            .iter()
            .filter(|e| {
                !actual
                    .iter()
                    .any(|(n, v, h)| *n == e.name && *v == e.version && *h == e.package_hash)
            })
            .map(|e| e.name.as_str())
            .collect()
    }
}

fn validate_accepted_assumptions(entry: &LockfileEntry, issues: &mut Vec<LockfileValidationIssue>) {
    let mut seen = BTreeSet::new();
    let mut duplicate_reported = BTreeSet::new();
    let mut empty_reported = false;
    let mut previous: Option<&str> = None;

    for assumption in &entry.accepted_assumptions {
        let assumption = assumption.as_str();
        if assumption.is_empty() {
            if !empty_reported {
                issues.push(LockfileValidationIssue::EmptyAcceptedAssumption {
                    name: entry.name.clone(),
                    version: entry.version.clone(),
                });
                empty_reported = true;
            }
            continue;
        }

        if let Some(previous) = previous {
            if assumption < previous {
                issues.push(LockfileValidationIssue::UnstableAcceptedAssumptionOrder {
                    name: entry.name.clone(),
                    version: entry.version.clone(),
                    previous: previous.to_string(),
                    assumption: assumption.to_string(),
                });
            }
        }
        previous = Some(assumption);

        if !seen.insert(assumption) && duplicate_reported.insert(assumption) {
            issues.push(LockfileValidationIssue::DuplicateAcceptedAssumption {
                name: entry.name.clone(),
                version: entry.version.clone(),
                assumption: assumption.to_string(),
            });
        }
    }
}

fn validate_artifact_evidence(entry: &LockfileEntry, issues: &mut Vec<LockfileValidationIssue>) {
    if has_artifact_role(&entry.artifact_hashes, "wasm-artifact")
        && !has_artifact_role(&entry.artifact_hashes, "wasm-abi-descriptor")
    {
        issues.push(LockfileValidationIssue::MissingAbiDescriptorArtifact {
            name: entry.name.clone(),
            version: entry.version.clone(),
        });
    }
}

fn has_artifact_role(artifact_hashes: &[ArtifactHashEntry], role: &str) -> bool {
    artifact_hashes
        .iter()
        .any(|entry| entry.role.trim() == role)
}

#[derive(Default)]
struct ResolvedPackageFacts {
    package_hashes: BTreeSet<String>,
    resolved_sources: BTreeSet<String>,
    expected_source_hashes: BTreeSet<String>,
    actual_source_hashes: BTreeSet<String>,
    graph_schemas: BTreeSet<Option<u32>>,
    core_ir_schemas: BTreeSet<Option<u32>>,
    has_missing_resolved_source: bool,
}

impl ResolvedPackageFacts {
    fn record(&mut self, package: &LockfileResolvedPackage) {
        self.package_hashes.insert(package.package_hash.clone());

        match package.resolved_source.as_deref() {
            Some(source) if !source.is_empty() => {
                self.resolved_sources.insert(source.to_string());
            }
            _ => self.has_missing_resolved_source = true,
        }

        if let Some(expected) = package.expected_source_hash.as_ref() {
            self.expected_source_hashes.insert(expected.clone());
        }
        if let Some(actual) = package.actual_source_hash.as_ref() {
            self.actual_source_hashes.insert(actual.clone());
        }

        self.graph_schemas.insert(package.graph_schema);
        self.core_ir_schemas.insert(package.core_ir_schema);
    }

    fn first_resolved_source(&self) -> Option<String> {
        self.resolved_sources.iter().next().cloned()
    }

    fn lowest_graph_schema(&self) -> Option<u32> {
        self.graph_schemas.iter().next().copied().flatten()
    }

    fn lowest_core_ir_schema(&self) -> Option<u32> {
        self.core_ir_schemas.iter().next().copied().flatten()
    }
}

fn integrity_issue_from_validation_issue(
    issue: &LockfileValidationIssue,
) -> LockfileIntegrityIssue {
    match issue {
        LockfileValidationIssue::UnstableEntryOrder {
            previous_name,
            previous_version,
            name,
            version,
        } => LockfileIntegrityIssue::new(
            LockfileIntegrityIssueKind::UnstableEntryOrder,
            name,
            version,
        )
        .with_expected_actual(
            format!("{previous_name}@{previous_version}"),
            format!("{name}@{version}"),
        ),
        LockfileValidationIssue::DuplicatePackageEntry { name, version } => {
            LockfileIntegrityIssue::new(
                LockfileIntegrityIssueKind::DuplicateLockfilePackage,
                name,
                version,
            )
        }
        LockfileValidationIssue::DuplicateActualPackage { name, version } => {
            LockfileIntegrityIssue::new(
                LockfileIntegrityIssueKind::DuplicateResolvedPackage,
                name,
                version,
            )
        }
        LockfileValidationIssue::MissingPackage { name, version } => {
            LockfileIntegrityIssue::new(LockfileIntegrityIssueKind::MissingPackage, name, version)
        }
        LockfileValidationIssue::PackageHashMismatch {
            name,
            version,
            expected,
            actual,
        }
        | LockfileValidationIssue::ArtifactHashMismatch {
            name,
            version,
            expected,
            actual,
        } => LockfileIntegrityIssue::new(
            if matches!(issue, LockfileValidationIssue::ArtifactHashMismatch { .. }) {
                LockfileIntegrityIssueKind::ArtifactHashMismatch
            } else {
                LockfileIntegrityIssueKind::PackageHashMismatch
            },
            name,
            version,
        )
        .with_expected_actual(expected, actual),
        LockfileValidationIssue::EmptyPackageHash { name, version } => {
            LockfileIntegrityIssue::new(LockfileIntegrityIssueKind::EmptyPackageHash, name, version)
        }
        LockfileValidationIssue::EmptyVerificationReportHash { name, version } => {
            LockfileIntegrityIssue::new(
                LockfileIntegrityIssueKind::EmptyVerificationReportHash,
                name,
                version,
            )
        }
        LockfileValidationIssue::MissingAbiDescriptorArtifact { name, version } => {
            LockfileIntegrityIssue::new(
                LockfileIntegrityIssueKind::MissingAbiDescriptorArtifact,
                name,
                version,
            )
        }
        LockfileValidationIssue::EmptyAcceptedAssumption { name, version } => {
            LockfileIntegrityIssue::new(
                LockfileIntegrityIssueKind::EmptyAcceptedAssumption,
                name,
                version,
            )
        }
        LockfileValidationIssue::DuplicateAcceptedAssumption {
            name,
            version,
            assumption,
        } => LockfileIntegrityIssue::new(
            LockfileIntegrityIssueKind::DuplicateAcceptedAssumption,
            name,
            version,
        )
        .with_expected_actual(assumption, assumption),
        LockfileValidationIssue::UnstableAcceptedAssumptionOrder {
            name,
            version,
            previous,
            assumption,
        } => LockfileIntegrityIssue::new(
            LockfileIntegrityIssueKind::UnstableAcceptedAssumptionOrder,
            name,
            version,
        )
        .with_expected_actual(previous, assumption),
    }
}

fn join_strings(values: &BTreeSet<String>) -> String {
    values.iter().cloned().collect::<Vec<_>>().join(",")
}

fn canonical_artifact_hashes(artifact_hashes: &[ArtifactHashEntry]) -> String {
    artifact_hashes
        .iter()
        .map(|entry| format!("{}={}", entry.role.trim(), entry.hash.trim()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(",")
}

fn sort_integrity_issues(issues: &mut [LockfileIntegrityIssue]) {
    issues.sort_by(|a, b| integrity_issue_sort_key(a).cmp(&integrity_issue_sort_key(b)));
}

fn integrity_issue_sort_key(
    issue: &LockfileIntegrityIssue,
) -> (String, String, String, String, String) {
    (
        issue.code.clone(),
        issue.package.name.clone(),
        issue.package.version.clone(),
        issue.expected.clone().unwrap_or_default(),
        issue.actual.clone().unwrap_or_default(),
    )
}

fn sort_validation_issues(issues: &mut [LockfileValidationIssue]) {
    issues.sort_by(|a, b| validation_issue_sort_key(a).cmp(&validation_issue_sort_key(b)));
}

fn validation_issue_sort_key(
    issue: &LockfileValidationIssue,
) -> (&'static str, &str, &str, &str, &str) {
    match issue {
        LockfileValidationIssue::UnstableEntryOrder {
            previous_name,
            previous_version,
            name,
            version,
        } => (issue.code(), name, version, previous_name, previous_version),
        LockfileValidationIssue::DuplicatePackageEntry { name, version }
        | LockfileValidationIssue::DuplicateActualPackage { name, version }
        | LockfileValidationIssue::MissingPackage { name, version }
        | LockfileValidationIssue::EmptyPackageHash { name, version }
        | LockfileValidationIssue::EmptyVerificationReportHash { name, version }
        | LockfileValidationIssue::MissingAbiDescriptorArtifact { name, version }
        | LockfileValidationIssue::EmptyAcceptedAssumption { name, version } => {
            (issue.code(), name, version, "", "")
        }
        LockfileValidationIssue::PackageHashMismatch {
            name,
            version,
            expected,
            actual,
        }
        | LockfileValidationIssue::ArtifactHashMismatch {
            name,
            version,
            expected,
            actual,
        } => (issue.code(), name, version, expected, actual),
        LockfileValidationIssue::DuplicateAcceptedAssumption {
            name,
            version,
            assumption,
        } => (issue.code(), name, version, assumption, ""),
        LockfileValidationIssue::UnstableAcceptedAssumptionOrder {
            name,
            version,
            previous,
            assumption,
        } => (issue.code(), name, version, previous, assumption),
    }
}

fn redacted_resolved_source(raw: &str) -> String {
    if raw.is_empty() {
        return "<redacted>".to_string();
    }

    if let Some(scheme_end) = raw.find("://") {
        let scheme = &raw[..scheme_end];
        let rest = &raw[scheme_end + 3..];
        let authority = rest
            .split(|ch| matches!(ch, '/' | '?' | '#'))
            .next()
            .unwrap_or_default();
        let authority = authority
            .rsplit_once('@')
            .map(|(_, host)| host)
            .unwrap_or(authority);
        if authority.is_empty() {
            format!("{scheme}://<redacted>")
        } else {
            format!("{scheme}://{authority}/<redacted>")
        }
    } else {
        "<redacted>".to_string()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> LockfileEntry {
        LockfileEntry {
            name: "payments.stripe".to_string(),
            version: "2.3.1".to_string(),
            requested_version: Some("^2.0".to_string()),
            package_hash: "a".repeat(64),
            trust_level: TrustLevel::Assumed,
            verification_report_hash: Some("b".repeat(64)),
            artifact_hashes: vec![
                ArtifactHashEntry {
                    role: "wasm-artifact".to_string(),
                    hash: "c".repeat(64),
                },
                ArtifactHashEntry {
                    role: "wasm-abi-descriptor".to_string(),
                    hash: "d".repeat(64),
                },
            ],
            accepted_assumptions: vec!["assume-pci".to_string(), "assume-gdpr".to_string()],
        }
    }

    fn entry_with(name: &str, version: &str, hash: &str) -> LockfileEntry {
        LockfileEntry {
            name: name.to_string(),
            version: version.to_string(),
            requested_version: None,
            package_hash: hash.to_string(),
            trust_level: TrustLevel::Verified,
            verification_report_hash: None,
            artifact_hashes: vec![],
            accepted_assumptions: vec![],
        }
    }

    // ── lockfile_entry_cbor_round_trip ────────────────────────────────────
    // Spec scenario: "CBOR round-trip preserves all fields"
    //   GIVEN a LockfileEntry with all fields set
    //   WHEN serialized to CBOR and deserialized
    //   THEN all fields are equal to the original
    #[test]
    fn lockfile_entry_cbor_round_trip() {
        let original = sample_entry();

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&original, &mut buf).expect("CBOR serialization must succeed");

        let decoded: LockfileEntry =
            ciborium::de::from_reader(buf.as_slice()).expect("CBOR deserialization must succeed");

        assert_eq!(decoded, original, "decoded entry must equal the original");
    }

    // ── lockfile_entry_cbor_is_deterministic ──────────────────────────────
    // TRIANGULATE: encoding the same value twice produces identical bytes.
    #[test]
    fn lockfile_entry_cbor_is_deterministic() {
        let entry = sample_entry();

        let mut buf1 = Vec::new();
        ciborium::ser::into_writer(&entry, &mut buf1).expect("first encode");

        let mut buf2 = Vec::new();
        ciborium::ser::into_writer(&entry, &mut buf2).expect("second encode");

        assert_eq!(
            buf1, buf2,
            "identical inputs must produce identical CBOR bytes"
        );
    }

    // ── lockfile_entry_without_report_hash ────────────────────────────────
    // TRIANGULATE: None verification_report_hash survives round-trip.
    #[test]
    fn lockfile_entry_without_report_hash() {
        let entry = LockfileEntry {
            verification_report_hash: None,
            artifact_hashes: vec![],
            accepted_assumptions: vec![],
            ..sample_entry()
        };

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&entry, &mut buf).expect("encode");
        let decoded: LockfileEntry = ciborium::de::from_reader(buf.as_slice()).expect("decode");

        assert_eq!(decoded.verification_report_hash, None);
        assert!(decoded.accepted_assumptions.is_empty());
    }

    // ── lockfile_add_and_get ──────────────────────────────────────────────
    // Spec scenario: "Lockfile can store and retrieve entries"
    //   GIVEN a Lockfile with one entry added
    //   WHEN get() is called with the same name/version
    //   THEN returns Some(&entry)
    #[test]
    fn lockfile_add_and_get() {
        let mut lf = Lockfile::new();
        assert!(lf.is_empty());

        lf.add(sample_entry());

        assert_eq!(lf.len(), 1);
        let found = lf.get("payments.stripe", "2.3.1");
        assert!(found.is_some());
        assert_eq!(found.unwrap().package_hash, "a".repeat(64));
    }

    // ── lockfile_cbor_round_trip ──────────────────────────────────────────
    // Spec scenario: "Lockfile round-trips through CBOR"
    #[test]
    fn lockfile_cbor_round_trip() {
        let mut lf = Lockfile::new();
        lf.add(sample_entry());
        lf.add(LockfileEntry {
            name: "utils.core".to_string(),
            version: "1.0.0".to_string(),
            requested_version: None,
            package_hash: "c".repeat(64),
            trust_level: TrustLevel::Verified,
            verification_report_hash: None,
            artifact_hashes: vec![],
            accepted_assumptions: vec![],
        });

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&lf, &mut buf).expect("encode");
        let decoded: Lockfile = ciborium::de::from_reader(buf.as_slice()).expect("decode");

        assert_eq!(decoded, lf);
    }

    // ── lockfile_is_hash_bound ────────────────────────────────────────────
    // Spec scenario: "Lockfile is content-addressed"
    //   GIVEN a Lockfile with entries
    //   WHEN blake3_hex() is called twice
    //   THEN both calls return identical 64-char hex strings
    #[test]
    fn lockfile_is_hash_bound() {
        let mut lf = Lockfile::new();
        lf.add(sample_entry());
        let h1 = lf.blake3_hex().expect("hash");
        let h2 = lf.blake3_hex().expect("hash");
        assert_eq!(h1.len(), 64);
        assert_eq!(h1, h2);
    }

    // ── lockfile_hash_changes_with_entries ────────────────────────────────
    // TRIANGULATE: adding an entry changes the lockfile hash
    #[test]
    fn lockfile_hash_changes_with_entries() {
        let lf1 = Lockfile::new();
        let mut lf2 = Lockfile::new();
        lf2.add(sample_entry());
        assert_ne!(lf1.blake3_hex().unwrap(), lf2.blake3_hex().unwrap());
    }

    // ── lockfile_verify_integrity_passes ─────────────────────────────────
    // Spec scenario: "Lockfile integrity check passes when all hashes match"
    #[test]
    fn lockfile_verify_integrity_passes() {
        let mut lf = Lockfile::new();
        lf.add(sample_entry());

        let hash = "a".repeat(64);
        let actual = vec![("payments.stripe", "2.3.1", hash.as_str())];
        let mismatches = lf.verify_integrity(&actual);
        assert!(mismatches.is_empty(), "all hashes match — no mismatches");
    }

    // ── lockfile_verify_integrity_detects_mismatch ────────────────────────
    // Spec scenario: "Integrity check detects hash mismatch"
    #[test]
    fn lockfile_verify_integrity_detects_mismatch() {
        let mut lf = Lockfile::new();
        lf.add(sample_entry());

        let wrong_hash = "z".repeat(64);
        let actual = vec![("payments.stripe", "2.3.1", wrong_hash.as_str())];
        let mismatches = lf.verify_integrity(&actual);
        assert_eq!(mismatches, vec!["payments.stripe"]);
    }

    // ── lockfile_validate_reproducibility_passes_for_canonical_lock ──────
    // Spec scenario: "Canonical lockfile validates against actual artifacts"
    #[test]
    fn lockfile_validate_reproducibility_passes_for_canonical_lock() {
        let mut lf = Lockfile::new();
        lf.add(entry_with("pkg.a", "1.0.0", "a"));
        lf.add(entry_with("pkg.b", "2.0.0", "b"));

        let actual = vec![("pkg.a", "1.0.0", "a"), ("pkg.b", "2.0.0", "b")];

        assert_eq!(lf.validate_reproducibility(&actual), vec![]);
    }

    // ── lockfile_validate_reproducibility_detects_unstable_order ─────────
    // Spec scenario: "Lockfile validation rejects non-canonical order"
    #[test]
    fn lockfile_validate_reproducibility_detects_unstable_order() {
        let mut lf = Lockfile::new();
        lf.add(entry_with("pkg.z", "1.0.0", "z"));
        lf.add(entry_with("pkg.a", "1.0.0", "a"));

        let actual = vec![("pkg.z", "1.0.0", "z"), ("pkg.a", "1.0.0", "a")];

        assert_eq!(
            lf.validate_reproducibility(&actual),
            vec![LockfileValidationIssue::UnstableEntryOrder {
                previous_name: "pkg.z".to_string(),
                previous_version: "1.0.0".to_string(),
                name: "pkg.a".to_string(),
                version: "1.0.0".to_string(),
            }]
        );
    }

    // ── lockfile_validate_reproducibility_detects_duplicate_lock_entry ───
    // Spec scenario: "Lockfile validation rejects duplicate package pins"
    #[test]
    fn lockfile_validate_reproducibility_detects_duplicate_lock_entry() {
        let mut lf = Lockfile::new();
        lf.add(entry_with("pkg.a", "1.0.0", "a"));
        lf.add(entry_with("pkg.a", "1.0.0", "a"));

        let actual = vec![("pkg.a", "1.0.0", "a")];

        assert_eq!(
            lf.validate_reproducibility(&actual),
            vec![LockfileValidationIssue::DuplicatePackageEntry {
                name: "pkg.a".to_string(),
                version: "1.0.0".to_string(),
            }]
        );
    }

    // ── lockfile_validate_reproducibility_detects_actual_digest_mismatch ─
    // Spec scenario: "Lockfile validation rejects actual package digest drift"
    #[test]
    fn lockfile_validate_reproducibility_detects_actual_digest_mismatch() {
        let mut lf = Lockfile::new();
        lf.add(entry_with("pkg.a", "1.0.0", "expected"));

        let actual = vec![("pkg.a", "1.0.0", "actual")];

        assert_eq!(
            lf.validate_reproducibility(&actual),
            vec![LockfileValidationIssue::PackageHashMismatch {
                name: "pkg.a".to_string(),
                version: "1.0.0".to_string(),
                expected: "expected".to_string(),
                actual: "actual".to_string(),
            }]
        );
    }

    // ── lockfile_validate_reproducibility_detects_missing_actual_package ─
    // Spec scenario: "Lockfile validation rejects missing actual package"
    #[test]
    fn lockfile_validate_reproducibility_detects_missing_actual_package() {
        let mut lf = Lockfile::new();
        lf.add(entry_with("pkg.a", "1.0.0", "a"));

        assert_eq!(
            lf.validate_reproducibility(&[]),
            vec![LockfileValidationIssue::MissingPackage {
                name: "pkg.a".to_string(),
                version: "1.0.0".to_string(),
            }]
        );
    }

    // ── lockfile_validate_reproducibility_detects_duplicate_actual_package
    // Spec scenario: "Lockfile validation rejects ambiguous actual artifacts"
    #[test]
    fn lockfile_validate_reproducibility_detects_duplicate_actual_package() {
        let mut lf = Lockfile::new();
        lf.add(entry_with("pkg.a", "1.0.0", "a"));

        let actual = vec![("pkg.a", "1.0.0", "a"), ("pkg.a", "1.0.0", "a")];

        assert_eq!(
            lf.validate_reproducibility(&actual),
            vec![LockfileValidationIssue::DuplicateActualPackage {
                name: "pkg.a".to_string(),
                version: "1.0.0".to_string(),
            }]
        );
    }

    // ── lockfile_validation_issue_exposes_stable_code_and_category ───────
    // Production gate: replay tools need stable machine-readable grouping.
    #[test]
    fn lockfile_validation_issue_exposes_stable_code_and_category() {
        let issue = LockfileValidationIssue::PackageHashMismatch {
            name: "pkg.a".to_string(),
            version: "1.0.0".to_string(),
            expected: "expected".to_string(),
            actual: "actual".to_string(),
        };

        assert_eq!(
            issue.kind(),
            LockfileValidationIssueKind::PackageHashMismatch
        );
        assert_eq!(issue.code(), "LOCKFILE_PACKAGE_HASH_MISMATCH");
        assert_eq!(
            issue.category(),
            LockfileValidationCategory::ReplayIntegrity
        );
        assert_eq!(issue.category().to_string(), "replay_integrity");
    }

    // ── lockfile_validate_reproducibility_requires_wasm_abi_descriptor ───
    // Production gate: a locked WASM binary is not replay-safe without its ABI contract.
    #[test]
    fn lockfile_validate_reproducibility_requires_wasm_abi_descriptor() {
        let mut lf = Lockfile::new();
        let mut entry = entry_with("abi.locked", "1.0.0", "hash");
        entry.artifact_hashes = vec![ArtifactHashEntry {
            role: "wasm-artifact".to_string(),
            hash: "a".repeat(64),
        }];
        lf.add(entry);

        let actual = vec![("abi.locked", "1.0.0", "hash")];

        let issue = LockfileValidationIssue::MissingAbiDescriptorArtifact {
            name: "abi.locked".to_string(),
            version: "1.0.0".to_string(),
        };
        assert_eq!(lf.validate_reproducibility(&actual), vec![issue.clone()]);
        assert_eq!(
            issue.kind(),
            LockfileValidationIssueKind::MissingAbiDescriptorArtifact
        );
        assert_eq!(issue.code(), "LOCKFILE_MISSING_ABI_DESCRIPTOR_ARTIFACT");
        assert_eq!(
            issue.category(),
            LockfileValidationCategory::LockfileIntegrity
        );
    }

    // ── lockfile_validate_artifact_reproducibility_detects_artifact_drift
    // Production gate: replay must compare the lock-visible artifact contract too.
    #[test]
    fn lockfile_validate_artifact_reproducibility_detects_artifact_drift() {
        let mut lf = Lockfile::new();
        let mut entry = entry_with("abi.locked", "1.0.0", "hash");
        entry.artifact_hashes = vec![
            ArtifactHashEntry {
                role: "wasm-artifact".to_string(),
                hash: "a".repeat(64),
            },
            ArtifactHashEntry {
                role: "wasm-abi-descriptor".to_string(),
                hash: "b".repeat(64),
            },
        ];
        lf.add(entry);

        let issues = lf.validate_artifact_reproducibility(&[LockfileArtifactEvidence::new(
            "abi.locked",
            "1.0.0",
            vec![
                ArtifactHashEntry {
                    role: "wasm-artifact".to_string(),
                    hash: "c".repeat(64),
                },
                ArtifactHashEntry {
                    role: "wasm-abi-descriptor".to_string(),
                    hash: "b".repeat(64),
                },
            ],
        )]);

        assert_eq!(issues.len(), 1);
        let issue = &issues[0];
        assert_eq!(
            issue.kind(),
            LockfileValidationIssueKind::ArtifactHashMismatch
        );
        assert_eq!(issue.code(), "LOCKFILE_ARTIFACT_HASH_MISMATCH");
        assert_eq!(
            issue.category(),
            LockfileValidationCategory::ReplayIntegrity
        );
    }

    // ── lockfile_validate_reproducibility_detects_empty_hashes ────────────
    // Production gate: replay cannot trust empty digest fields.
    #[test]
    fn lockfile_validate_reproducibility_detects_empty_hashes() {
        let mut lf = Lockfile::new();
        let mut entry = entry_with("pkg.empty", "1.0.0", "");
        entry.verification_report_hash = Some("".to_string());
        lf.add(entry);

        let actual = vec![("pkg.empty", "1.0.0", "")];

        assert_eq!(
            lf.validate_reproducibility(&actual),
            vec![
                LockfileValidationIssue::EmptyPackageHash {
                    name: "pkg.empty".to_string(),
                    version: "1.0.0".to_string(),
                },
                LockfileValidationIssue::EmptyVerificationReportHash {
                    name: "pkg.empty".to_string(),
                    version: "1.0.0".to_string(),
                },
            ]
        );
    }

    // ── lockfile_validate_reproducibility_detects_assumption_drift ────────
    // Production gate: accepted assumptions are a canonical replay input.
    #[test]
    fn lockfile_validate_reproducibility_detects_assumption_drift() {
        let mut lf = Lockfile::new();
        let mut entry = entry_with("pkg.assumed", "1.0.0", "hash");
        entry.accepted_assumptions = vec![
            "assume-z".to_string(),
            "assume-a".to_string(),
            "assume-a".to_string(),
            "".to_string(),
        ];
        lf.add(entry);

        let actual = vec![("pkg.assumed", "1.0.0", "hash")];

        assert_eq!(
            lf.validate_reproducibility(&actual),
            vec![
                LockfileValidationIssue::DuplicateAcceptedAssumption {
                    name: "pkg.assumed".to_string(),
                    version: "1.0.0".to_string(),
                    assumption: "assume-a".to_string(),
                },
                LockfileValidationIssue::EmptyAcceptedAssumption {
                    name: "pkg.assumed".to_string(),
                    version: "1.0.0".to_string(),
                },
                LockfileValidationIssue::UnstableAcceptedAssumptionOrder {
                    name: "pkg.assumed".to_string(),
                    version: "1.0.0".to_string(),
                    previous: "assume-z".to_string(),
                    assumption: "assume-a".to_string(),
                },
            ]
        );
    }

    // ── lockfile_validate_reproducibility_orders_issues_deterministically ─
    // TRIANGULATE: issue order and duplicate actual hashes do not depend on caller order.
    #[test]
    fn lockfile_validate_reproducibility_orders_issues_deterministically() {
        let mut lf = Lockfile::new();
        let mut invalid = entry_with("pkg.a", "1.0.0", "");
        invalid.verification_report_hash = Some("".to_string());
        lf.add(invalid);
        let mut assumed = entry_with("pkg.b", "1.0.0", "expected");
        assumed.accepted_assumptions = vec!["z".to_string(), "a".to_string(), "a".to_string()];
        lf.add(assumed);

        let actual_1 = vec![
            ("pkg.b", "1.0.0", "y"),
            ("pkg.a", "1.0.0", ""),
            ("pkg.b", "1.0.0", "x"),
        ];
        let actual_2 = vec![
            ("pkg.b", "1.0.0", "x"),
            ("pkg.b", "1.0.0", "y"),
            ("pkg.a", "1.0.0", ""),
        ];

        let issues = lf.validate_reproducibility(&actual_1);
        assert_eq!(issues, lf.validate_reproducibility(&actual_2));
        assert_eq!(
            issues.iter().map(|issue| issue.code()).collect::<Vec<_>>(),
            vec![
                "LOCKFILE_DUPLICATE_ACCEPTED_ASSUMPTION",
                "LOCKFILE_DUPLICATE_ACTUAL_PACKAGE",
                "LOCKFILE_EMPTY_PACKAGE_HASH",
                "LOCKFILE_EMPTY_VERIFICATION_REPORT_HASH",
                "LOCKFILE_PACKAGE_HASH_MISMATCH",
                "LOCKFILE_UNSTABLE_ACCEPTED_ASSUMPTION_ORDER",
            ]
        );
        assert!(matches!(
            &issues[4],
            LockfileValidationIssue::PackageHashMismatch { actual, .. } if actual == "x,y"
        ));
    }

    // ── lockfile_diagnose_integrity_reports_production_metadata ──────────
    // Production gate: replay diagnostics expose stable issue codes for package
    // hash/source drift, duplicate packages, missing source descriptors, stale
    // schemas, and deterministic issue ordering.
    #[test]
    fn lockfile_diagnose_integrity_reports_production_metadata() {
        let mut lf = Lockfile::new();
        lf.add(entry_with("pkg.a", "1.0.0", "locked-package"));
        lf.add(entry_with("pkg.b", "1.0.0", "b"));

        let requirements = LockfileValidationRequirements {
            require_resolved_source: true,
            min_graph_schema: Some(3),
            min_core_ir_schema: Some(2),
        };
        let actual_1 = vec![
            LockfileResolvedPackage::new("pkg.b", "1.0.0", "b")
                .with_schema_versions(Some(3), Some(2)),
            LockfileResolvedPackage::new("pkg.a", "1.0.0", "actual-package")
                .with_resolved_source(
                    "https://user:secret@registry.example.test/packages/pkg.a?token=secret#frag",
                )
                .with_source_hashes("locked-source", "actual-source")
                .with_schema_versions(Some(1), None),
            LockfileResolvedPackage::new("pkg.a", "1.0.0", "actual-package")
                .with_resolved_source("https://registry.example.test/packages/pkg.a")
                .with_source_hashes("locked-source", "actual-source")
                .with_schema_versions(Some(1), None),
        ];
        let actual_2 = actual_1.iter().cloned().rev().collect::<Vec<_>>();

        let issues = lf.diagnose_integrity(&actual_1, &requirements);

        assert_eq!(
            issues,
            lf.diagnose_integrity(&actual_2, &requirements),
            "diagnostics must not depend on replay descriptor order"
        );
        assert_eq!(
            issues
                .iter()
                .map(|issue| issue.code.as_str())
                .collect::<Vec<_>>(),
            vec![
                "LOCKFILE_DUPLICATE_RESOLVED_PACKAGE",
                "LOCKFILE_MISSING_RESOLVED_SOURCE",
                "LOCKFILE_PACKAGE_HASH_MISMATCH",
                "LOCKFILE_SOURCE_HASH_MISMATCH",
                "LOCKFILE_STALE_CORE_IR_SCHEMA_VERSION",
                "LOCKFILE_STALE_GRAPH_SCHEMA_VERSION",
            ]
        );
        assert_eq!(
            issues
                .iter()
                .map(|issue| issue.category)
                .collect::<Vec<_>>(),
            vec![
                LockfileValidationCategory::Determinism,
                LockfileValidationCategory::LockfileIntegrity,
                LockfileValidationCategory::ReplayIntegrity,
                LockfileValidationCategory::ReplayIntegrity,
                LockfileValidationCategory::LockfileIntegrity,
                LockfileValidationCategory::LockfileIntegrity,
            ]
        );
    }

    // ── lockfile_diagnostics_redact_resolved_source_descriptors ──────────
    // Production gate: package descriptors expose source shape without leaking
    // URL credentials, path, query token, or fragment.
    #[test]
    fn lockfile_diagnostics_redact_resolved_source_descriptors() {
        let mut lf = Lockfile::new();
        lf.add(entry_with("pkg.secret", "1.0.0", "package"));
        let actual = vec![
            LockfileResolvedPackage::new("pkg.secret", "1.0.0", "package")
                .with_resolved_source(
                    "https://user:pass@registry.example.test/private/pkg?token=abc#frag",
                )
                .with_source_hashes("locked-source", "actual-source"),
        ];

        let issues = lf.diagnose_integrity(&actual, &LockfileValidationRequirements::default());
        let source_issue = issues
            .iter()
            .find(|issue| issue.kind == LockfileIntegrityIssueKind::SourceHashMismatch)
            .expect("source mismatch issue");

        assert_eq!(source_issue.package.name, "pkg.secret");
        assert_eq!(source_issue.package.version, "1.0.0");
        assert_eq!(
            source_issue.package.resolved_source.as_deref(),
            Some("https://registry.example.test/<redacted>")
        );
        assert!(source_issue.package.redacted);
        assert!(
            !format!("{source_issue:?}").contains("token=abc"),
            "diagnostic must not leak source URL secrets"
        );
    }

    // ── lockfile_get_returns_none_for_missing ─────────────────────────────
    // TRIANGULATE: lookup of absent package returns None
    #[test]
    fn lockfile_get_returns_none_for_missing() {
        let lf = Lockfile::new();
        assert!(lf.get("unknown", "1.0.0").is_none());
    }

    // ── B5: Lockfile::from_resolution and to_specs ────────────────────────

    use crate::manifest::{PackageDef, PackageManifest};
    use crate::resolver::DependencySpec;

    fn make_test_manifest(name: &str, version: &str) -> PackageManifest {
        PackageManifest::from_def(PackageDef {
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
            provenance: None,
            verification_report: None,
            graph_schema: None,
            core_ir_schema: None,
            // 4G fields
            reproducible_evidence: None,
        })
    }

    fn make_test_spec(name: &str, version: &str) -> DependencySpec {
        DependencySpec {
            name: name.to_string(),
            version_constraint: version.to_string(),
            min_trust: TrustLevel::Unverified,
            profile: None,
            allowed_licenses: vec![],
            denied_capabilities: vec![],
            denied_handlers: vec![],
            min_graph_schema: None,
            min_core_ir_schema: None,
        }
    }

    // Spec PKG-LOCK-1: from_resolution() builds entry with pinned version
    #[test]
    fn from_resolution_builds_pinned_entry() {
        let manifest = make_test_manifest("payments.stripe", "2.3.1");
        let spec = make_test_spec("payments.stripe", "^2.0");
        let lf = Lockfile::from_resolution(vec![(&spec, &manifest)]);

        assert_eq!(lf.len(), 1);
        let entry = lf
            .get("payments.stripe", "2.3.1")
            .expect("entry must exist");
        assert_eq!(entry.name, "payments.stripe");
        assert_eq!(
            entry.version, "2.3.1",
            "version must be pinned from manifest"
        );
        assert_eq!(entry.trust_level, TrustLevel::Verified);
        assert_eq!(entry.requested_version.as_deref(), Some("^2.0"));
    }

    #[test]
    fn from_resolution_pins_verification_report_hash() {
        let mut manifest = make_test_manifest("payments.stripe", "2.3.1");
        manifest.verification_report = Some(crate::verification::PackageVerificationReport {
            package: "payments.stripe".to_string(),
            version: "2.3.1".to_string(),
            exports_verified: vec!["charge".to_string()],
            effects_declared: vec![],
            assumptions: vec![],
            unsafe_surface: vec![],
            artifact_hashes: vec!["a".repeat(64)],
        });
        let expected = manifest
            .verification_report
            .as_ref()
            .expect("report must exist")
            .blake3_hex()
            .expect("report hash must compute");
        let spec = make_test_spec("payments.stripe", "^2.0");

        let lf = Lockfile::from_resolution(vec![(&spec, &manifest)]);

        let entry = lf
            .get("payments.stripe", "2.3.1")
            .expect("entry must exist");
        assert_eq!(
            entry.verification_report_hash.as_deref(),
            Some(expected.as_str())
        );
    }

    #[test]
    fn from_resolution_pins_manifest_artifact_hashes() {
        let mut manifest = make_test_manifest("payments.stripe", "2.3.1");
        manifest.artifact_hashes = vec![
            ArtifactHashEntry {
                role: "wasm-artifact".to_string(),
                hash: "a".repeat(64),
            },
            ArtifactHashEntry {
                role: "wasm-abi-descriptor".to_string(),
                hash: "b".repeat(64),
            },
        ];
        let spec = make_test_spec("payments.stripe", "^2.0");

        let lf = Lockfile::from_resolution(vec![(&spec, &manifest)]);

        let entry = lf
            .get("payments.stripe", "2.3.1")
            .expect("entry must exist");
        assert_eq!(entry.artifact_hashes, manifest.artifact_hashes);
    }

    // Spec PKG-LOCK-1: to_specs() returns exact-version DependencySpecs
    #[test]
    fn to_specs_returns_exact_version_specs() {
        let manifest = make_test_manifest("payments.stripe", "2.3.1");
        let spec = make_test_spec("payments.stripe", "^2.0");
        let lf = Lockfile::from_resolution(vec![(&spec, &manifest)]);

        let specs = lf.to_specs();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "payments.stripe");
        assert_eq!(
            specs[0].version_constraint, "2.3.1",
            "to_specs must pin exact version"
        );
        assert_eq!(specs[0].min_trust, TrustLevel::Unverified);
    }

    // Spec PKG-LOCK-1: multiple entries round-trip through from_resolution/to_specs
    #[test]
    fn from_resolution_multiple_entries() {
        let m1 = make_test_manifest("pkg.a", "1.0.0");
        let m2 = make_test_manifest("pkg.b", "2.5.0");
        let s1 = make_test_spec("pkg.a", "^1.0");
        let s2 = make_test_spec("pkg.b", ">=2.0");
        let lf = Lockfile::from_resolution(vec![(&s1, &m1), (&s2, &m2)]);

        assert_eq!(lf.len(), 2);
        let specs = lf.to_specs();
        assert_eq!(specs.len(), 2);
        // Pinned exactly
        assert!(
            specs
                .iter()
                .any(|s| s.name == "pkg.a" && s.version_constraint == "1.0.0")
        );
        assert!(
            specs
                .iter()
                .any(|s| s.name == "pkg.b" && s.version_constraint == "2.5.0")
        );
    }

    // Spec PKG-LOCK-REPRO-1: from_resolution() canonicalizes package order
    #[test]
    fn from_resolution_is_independent_of_input_order() {
        let m1 = make_test_manifest("pkg.a", "1.0.0");
        let m2 = make_test_manifest("pkg.b", "2.5.0");
        let s1 = make_test_spec("pkg.a", "^1.0");
        let s2 = make_test_spec("pkg.b", ">=2.0");

        let lf1 = Lockfile::from_resolution(vec![(&s1, &m1), (&s2, &m2)]);
        let lf2 = Lockfile::from_resolution(vec![(&s2, &m2), (&s1, &m1)]);

        assert_eq!(lf1.entries[0].name, "pkg.a");
        assert_eq!(lf1.entries[1].name, "pkg.b");
        assert_eq!(lf1, lf2);
        assert_eq!(lf1.blake3_hex().unwrap(), lf2.blake3_hex().unwrap());
    }
}
