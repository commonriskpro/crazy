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
//   package_hash
//   trust_level
//   verification_report_hash
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

use crate::manifest::PackageManifest;
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
    /// BLAKE3 hex digest of the package artifact at lock time.
    pub package_hash: String,
    /// Trust level recorded at lock time.
    pub trust_level: TrustLevel,
    /// Optional BLAKE3 hex digest of the verification report used to
    /// produce this lock entry.
    pub verification_report_hash: Option<String>,
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
    /// A locked package has no package artifact digest.
    EmptyPackageHash,
    /// A locked package records an empty verification report digest.
    EmptyVerificationReportHash,
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
            LockfileValidationIssueKind::EmptyPackageHash => "LOCKFILE_EMPTY_PACKAGE_HASH",
            LockfileValidationIssueKind::EmptyVerificationReportHash => {
                "LOCKFILE_EMPTY_VERIFICATION_REPORT_HASH"
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
            | LockfileValidationIssueKind::EmptyAcceptedAssumption => {
                LockfileValidationCategory::LockfileIntegrity
            }
            LockfileValidationIssueKind::MissingPackage
            | LockfileValidationIssueKind::PackageHashMismatch => {
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
    /// A locked package has no package artifact digest.
    EmptyPackageHash { name: String, version: String },
    /// A locked package records an empty verification report digest.
    EmptyVerificationReportHash { name: String, version: String },
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
            LockfileValidationIssue::EmptyPackageHash { .. } => {
                LockfileValidationIssueKind::EmptyPackageHash
            }
            LockfileValidationIssue::EmptyVerificationReportHash { .. } => {
                LockfileValidationIssueKind::EmptyVerificationReportHash
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
            .map(|(_spec, manifest)| {
                let package_hash = manifest.blake3_hex().unwrap_or_default();
                let verification_report_hash = manifest
                    .verification_report
                    .as_ref()
                    .and_then(|report| report.blake3_hex().ok());
                LockfileEntry {
                    name: manifest.name.clone(),
                    version: manifest.version.clone(),
                    package_hash,
                    trust_level: manifest.trust_level,
                    verification_report_hash,
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
        | LockfileValidationIssue::EmptyAcceptedAssumption { name, version } => {
            (issue.code(), name, version, "", "")
        }
        LockfileValidationIssue::PackageHashMismatch {
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

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> LockfileEntry {
        LockfileEntry {
            name: "payments.stripe".to_string(),
            version: "2.3.1".to_string(),
            package_hash: "a".repeat(64),
            trust_level: TrustLevel::Assumed,
            verification_report_hash: Some("b".repeat(64)),
            accepted_assumptions: vec!["assume-pci".to_string(), "assume-gdpr".to_string()],
        }
    }

    fn entry_with(name: &str, version: &str, hash: &str) -> LockfileEntry {
        LockfileEntry {
            name: name.to_string(),
            version: version.to_string(),
            package_hash: hash.to_string(),
            trust_level: TrustLevel::Verified,
            verification_report_hash: None,
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
            package_hash: "c".repeat(64),
            trust_level: TrustLevel::Verified,
            verification_report_hash: None,
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
