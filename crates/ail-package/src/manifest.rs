// ── ail-package::manifest ─────────────────────────────────────────────────
//
// `PackageManifest` — the canonical typed representation of one package.
//
// # Hash contract
//
// `PackageManifest::blake3_hex()` computes:
//   `BLAKE3(canonical_cbor(manifest))`
//
// where `canonical_cbor` is the deterministic `ciborium` CBOR encoding.
// All fields are deterministic types (no `HashMap`, no `f32`/`f64`).
//
// # Validation contract
//
// `PackageManifest::validate()` enforces structural invariants, notably:
// a package with `TrustLevel::Unsafe` MUST declare at least one
// `UnsafeSurfaceEntry`; otherwise the manifest is rejected.

use blake3::Hasher;
use ciborium::ser::into_writer;
use serde::{Deserialize, Serialize};

use crate::assumption::PackageAssumption;
use crate::surface::UnsafeSurfaceEntry;
use crate::trust::TrustLevel;

// ── ArtifactHashEntry ─────────────────────────────────────────────────────

/// One artifact hash in the reproducible-build metadata.
///
/// Binds an artifact role (e.g., `"wasm-binary"`, `"source-archive"`) to its
/// BLAKE3 hex digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactHashEntry {
    /// Role of this artifact (e.g., `"wasm-binary"`, `"source-archive"`).
    pub role: String,
    /// BLAKE3 hex digest of the artifact bytes (64 lower-case hex characters).
    pub hash: String,
}

// ── PackageDef ────────────────────────────────────────────────────────────

/// All fields required to construct a `PackageManifest`.
///
/// `PackageDef` is the builder input.  Pass it to [`PackageManifest::from_def`]
/// to obtain an immutable manifest ready for hashing and validation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageDef {
    /// Package name (e.g., `"payments.stripe"`).
    pub name: String,
    /// Semantic version string (e.g., `"2.3.1"`).
    pub version: String,
    /// Trust tier assigned to this package.
    pub trust_level: TrustLevel,
    /// Capability IDs required by this package (e.g., `"payment.charge"`).
    ///
    /// Importing this package does NOT automatically grant these capabilities
    /// to the importing module (`import != grant`).
    pub required_capabilities: Vec<String>,
    /// Capability IDs exported by this package for use by importers.
    pub exported_capabilities: Vec<String>,
    /// Documented assumptions attached to this package.
    pub assumptions: Vec<PackageAssumption>,
    /// Declared unsafe surface items (required for `TrustLevel::Unsafe`).
    pub unsafe_surface: Vec<UnsafeSurfaceEntry>,
    /// Reproducible-build artifact hashes.
    pub artifact_hashes: Vec<ArtifactHashEntry>,
    /// Optional BLAKE3 hex digest of the build environment snapshot.
    pub build_env_hash: Option<String>,
}

// ── PackageValidationError ────────────────────────────────────────────────

/// Errors produced by [`PackageManifest::validate`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageValidationError {
    /// A package with `TrustLevel::Unsafe` declared no unsafe surface items.
    ///
    /// Every `Unsafe` package must explicitly enumerate its unsafe surface so
    /// reviewers can inspect it.
    UnsafeWithoutSurface,
}

impl std::fmt::Display for PackageValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageValidationError::UnsafeWithoutSurface => write!(
                f,
                "package has TrustLevel::Unsafe but declares no unsafe surface items"
            ),
        }
    }
}

impl std::error::Error for PackageValidationError {}

// ── PackageError ──────────────────────────────────────────────────────────

/// Errors returned by [`PackageManifest::blake3_hex`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageError(pub String);

impl std::fmt::Display for PackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "package error: {}", self.0)
    }
}

impl std::error::Error for PackageError {}

// ── PackageManifest ───────────────────────────────────────────────────────

/// The canonical typed representation of one package.
///
/// Constructed via [`PackageManifest::from_def`]; immutable after construction.
/// The manifest is content-addressed: [`PackageManifest::blake3_hex`] returns
/// a deterministic BLAKE3 digest of the canonical CBOR encoding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageManifest {
    /// Package name (e.g., `"payments.stripe"`).
    pub name: String,
    /// Semantic version string.
    pub version: String,
    /// Trust tier assigned to this package.
    pub trust_level: TrustLevel,
    /// Capability IDs required by this package.
    ///
    /// Importing this package does NOT automatically grant these capabilities
    /// to the importing module (`import != grant`).
    pub required_capabilities: Vec<String>,
    /// Capability IDs exported by this package.
    pub exported_capabilities: Vec<String>,
    /// Documented assumptions attached to this package.
    pub assumptions: Vec<PackageAssumption>,
    /// Declared unsafe surface items.
    pub unsafe_surface: Vec<UnsafeSurfaceEntry>,
    /// Reproducible-build artifact hashes.
    pub artifact_hashes: Vec<ArtifactHashEntry>,
    /// Optional BLAKE3 hex digest of the build environment snapshot.
    pub build_env_hash: Option<String>,
}

impl PackageManifest {
    /// Construct a `PackageManifest` from a `PackageDef`.
    ///
    /// This is a direct field-for-field mapping; no validation is performed.
    /// Call [`PackageManifest::validate`] separately to enforce invariants.
    pub fn from_def(def: PackageDef) -> Self {
        PackageManifest {
            name: def.name,
            version: def.version,
            trust_level: def.trust_level,
            required_capabilities: def.required_capabilities,
            exported_capabilities: def.exported_capabilities,
            assumptions: def.assumptions,
            unsafe_surface: def.unsafe_surface,
            artifact_hashes: def.artifact_hashes,
            build_env_hash: def.build_env_hash,
        }
    }

    /// Compute the BLAKE3 content hash as a hex-encoded string.
    ///
    /// The hash covers the canonical CBOR serialization of the full manifest.
    /// Returns a 64-character lower-case hex string.
    ///
    /// # Errors
    ///
    /// Returns `Err(PackageError)` if CBOR serialization fails.
    pub fn blake3_hex(&self) -> Result<String, PackageError> {
        let mut buf = Vec::new();
        into_writer(self, &mut buf)
            .map_err(|e| PackageError(format!("CBOR serialization failed: {e}")))?;
        let mut hasher = Hasher::new();
        hasher.update(&buf);
        Ok(hasher.finalize().to_hex().to_string())
    }

    /// Validate structural invariants.
    ///
    /// Currently enforces: a package with `TrustLevel::Unsafe` must have at
    /// least one entry in `unsafe_surface`.
    ///
    /// # Errors
    ///
    /// Returns `Err(PackageValidationError::UnsafeWithoutSurface)` when the
    /// `Unsafe` × empty-surface invariant is violated.
    pub fn validate(&self) -> Result<(), PackageValidationError> {
        if self.trust_level == TrustLevel::Unsafe && self.unsafe_surface.is_empty() {
            return Err(PackageValidationError::UnsafeWithoutSurface);
        }
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_def(trust_level: TrustLevel) -> PackageDef {
        PackageDef {
            name: "test.package".to_string(),
            version: "1.0.0".to_string(),
            trust_level,
            required_capabilities: vec![],
            exported_capabilities: vec![],
            assumptions: vec![],
            unsafe_surface: vec![],
            artifact_hashes: vec![],
            build_env_hash: None,
        }
    }

    // ── verified_manifest_is_constructible ───────────────────────────────
    // Spec scenario: "Constructing a minimal verified package"
    //   GIVEN a PackageDef with trust_level: Verified and no capabilities or assumptions
    //   WHEN PackageManifest::from_def is called
    //   THEN the manifest is constructible and blake3_hex() returns a 64-char hex string
    #[test]
    fn verified_manifest_is_constructible() {
        let def = minimal_def(TrustLevel::Verified);
        let m = PackageManifest::from_def(def);
        let hex = m.blake3_hex().expect("blake3_hex must succeed");
        assert_eq!(hex.len(), 64, "BLAKE3 hex must be 64 characters");
    }

    // ── validate_rejects_unsafe_without_surface ───────────────────────────
    // Spec scenario: "Constructing an unsafe package requires surface declaration"
    //   GIVEN a PackageDef with trust_level: Unsafe and an empty unsafe_surface
    //   WHEN PackageManifest::validate() is called
    //   THEN it returns Err(PackageValidationError::UnsafeWithoutSurface)
    #[test]
    fn validate_rejects_unsafe_without_surface() {
        let m = PackageManifest::from_def(minimal_def(TrustLevel::Unsafe));
        assert_eq!(
            m.validate(),
            Err(PackageValidationError::UnsafeWithoutSurface)
        );
    }

    // ── validate_accepts_unsafe_with_surface ─────────────────────────────
    // TRIANGULATE: an Unsafe package WITH surface entries passes validation.
    #[test]
    fn validate_accepts_unsafe_with_surface() {
        let mut def = minimal_def(TrustLevel::Unsafe);
        def.unsafe_surface.push(UnsafeSurfaceEntry {
            kind: "ffi".to_string(),
            name: "libc::malloc".to_string(),
            description: "Raw allocation for performance-critical path.".to_string(),
        });
        let m = PackageManifest::from_def(def);
        assert_eq!(m.validate(), Ok(()));
    }

    // ── validate_accepts_verified_with_empty_surface ─────────────────────
    // TRIANGULATE: non-Unsafe packages pass validation with empty surface.
    #[test]
    fn validate_accepts_non_unsafe_with_empty_surface() {
        for level in [TrustLevel::Verified, TrustLevel::Assumed, TrustLevel::Unverified] {
            let m = PackageManifest::from_def(minimal_def(level));
            assert_eq!(m.validate(), Ok(()), "level {level} should pass validate");
        }
    }

    // ── blake3_hex_is_deterministic ───────────────────────────────────────
    // Spec scenario: "Deterministic hash"
    //   GIVEN two PackageManifest values with identical fields
    //   WHEN blake3_hex() is called on each
    //   THEN both return the same hex string
    #[test]
    fn blake3_hex_is_deterministic() {
        let def1 = minimal_def(TrustLevel::Verified);
        let def2 = minimal_def(TrustLevel::Verified);
        let m1 = PackageManifest::from_def(def1);
        let m2 = PackageManifest::from_def(def2);
        assert_eq!(
            m1.blake3_hex().unwrap(),
            m2.blake3_hex().unwrap(),
            "identical manifests must hash to the same value"
        );
    }

    // ── blake3_hex_differs_for_different_manifests ────────────────────────
    // Spec scenario: "Changed field produces different hash"
    //   GIVEN a manifest m1 and a copy m2 where m2.name differs
    //   WHEN blake3_hex() is called
    //   THEN m1.blake3_hex() != m2.blake3_hex()
    #[test]
    fn blake3_hex_differs_for_different_manifests() {
        let def1 = minimal_def(TrustLevel::Verified);
        let mut def2 = minimal_def(TrustLevel::Verified);
        def2.name = "other.package".to_string();

        let m1 = PackageManifest::from_def(def1);
        let m2 = PackageManifest::from_def(def2);

        assert_ne!(
            m1.blake3_hex().unwrap(),
            m2.blake3_hex().unwrap(),
            "manifests with different names must produce different hashes"
        );
    }
}
