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
    /// Assumption IDs accepted by the approver at lock time, in declaration order.
    ///
    /// Uses `Vec` (not `HashSet`) to maintain CBOR determinism.
    pub accepted_assumptions: Vec<String>,
}

// ── Lockfile ──────────────────────────────────────────────────────────────

/// A resolved and pinned dependency graph — the full lockfile.
///
/// A `Lockfile` is an ordered collection of `LockfileEntry` records.
/// It is produced by the dependency resolver after a successful resolution
/// run and can be used to reproduce the same graph deterministically.
///
/// The lockfile itself is content-addressed via
/// [`Lockfile::blake3_hex`] — hashing the canonical CBOR encoding of all
/// entries in insertion order.
///
/// See `docs/packages.md` §Reproducibility.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lockfile {
    /// Pinned package entries in resolution order.
    pub entries: Vec<LockfileEntry>,
}

impl Lockfile {
    /// Create an empty lockfile.
    pub fn new() -> Self {
        Lockfile::default()
    }

    /// Add a resolved entry to the lockfile.
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
    /// hashing fails the field is set to an empty string.
    pub fn from_resolution(resolutions: Vec<(&DependencySpec, &PackageManifest)>) -> Self {
        let entries = resolutions
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
}
