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
use crate::export::ExportDeclaration;
use crate::handler::HandlerExport;
use crate::import::ImportDeclaration;
use crate::surface::UnsafeSurfaceEntry;
use crate::trust::TrustLevel;
use crate::verification::PackageVerificationReport;

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
///
/// All fields added in G12 are `Option<T>` or `Vec<T>` (defaulting to empty)
/// to maintain backward compatibility with manifests produced before G12.
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_env_hash: Option<String>,

    // ── G12 fields ────────────────────────────────────────────────────────
    /// Handler implementations exported by this package.
    ///
    /// A handler export is NOT a binding — the runtime profile must
    /// explicitly bind the handler and grant the capability.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub handlers: Vec<HandlerExport>,
    /// Contract IDs declared at the package level
    /// (e.g., `["idempotent_by_key"]`).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub contracts: Vec<String>,
    /// Public export declarations for this package.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub exports: Vec<ExportDeclaration>,
    /// Import declarations listing dependencies brought into scope.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub imports: Vec<ImportDeclaration>,
    /// Trust boundaries within which this package operates
    /// (e.g., `["boundary.Stripe"]`).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub boundaries: Vec<String>,
    /// SPDX license identifier or expression (e.g., `"Apache-2.0"`, `"MIT OR Apache-2.0"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Provenance URL or identifier linking to the upstream source or build
    /// pipeline (e.g., a CI run URL or source archive URI).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
    /// Verification report summarizing the results of the package review.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_report: Option<PackageVerificationReport>,
    /// Graph schema version this package was compiled against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_schema: Option<u32>,
    /// Core IR schema version this package was compiled against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub core_ir_schema: Option<u32>,
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
    /// An `ExportDeclaration` has an empty `name` field.
    ///
    /// Every export must have a non-empty qualified name.
    ExportNameEmpty,
    /// A `HandlerExport` has an empty `capability` or `handler_name` field.
    ///
    /// Every handler export must identify both the capability it handles and
    /// the handler name.
    HandlerFieldEmpty,
    /// An `ImportDeclaration` has an empty `source_package` field.
    ///
    /// Every import must identify its source package.
    ImportSourceEmpty,
}

impl std::fmt::Display for PackageValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageValidationError::UnsafeWithoutSurface => write!(
                f,
                "package has TrustLevel::Unsafe but declares no unsafe surface items"
            ),
            PackageValidationError::ExportNameEmpty => {
                write!(f, "an export declaration has an empty name")
            }
            PackageValidationError::HandlerFieldEmpty => {
                write!(
                    f,
                    "a handler export has an empty capability or handler_name"
                )
            }
            PackageValidationError::ImportSourceEmpty => {
                write!(f, "an import declaration has an empty source_package")
            }
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
///
/// All fields added in G12 use `skip_serializing_if` so that manifests
/// serialized without those fields can still be deserialized (backward compat).
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_env_hash: Option<String>,

    // ── G12 fields ────────────────────────────────────────────────────────
    /// Handler implementations exported by this package.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub handlers: Vec<HandlerExport>,
    /// Contract IDs declared at the package level.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub contracts: Vec<String>,
    /// Public export declarations for this package.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub exports: Vec<ExportDeclaration>,
    /// Import declarations listing dependencies brought into scope.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub imports: Vec<ImportDeclaration>,
    /// Trust boundaries within which this package operates.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub boundaries: Vec<String>,
    /// SPDX license identifier or expression.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Provenance URL or identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
    /// Verification report summarizing the results of the package review.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_report: Option<PackageVerificationReport>,
    /// Graph schema version this package was compiled against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_schema: Option<u32>,
    /// Core IR schema version this package was compiled against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub core_ir_schema: Option<u32>,
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
            // G12 fields
            handlers: def.handlers,
            contracts: def.contracts,
            exports: def.exports,
            imports: def.imports,
            boundaries: def.boundaries,
            license: def.license,
            provenance: def.provenance,
            verification_report: def.verification_report,
            graph_schema: def.graph_schema,
            core_ir_schema: def.core_ir_schema,
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
    /// Enforces the following rules:
    ///
    /// 1. A package with `TrustLevel::Unsafe` must declare at least one
    ///    `unsafe_surface` entry.
    /// 2. Every `ExportDeclaration` must have a non-empty `name`.
    /// 3. Every `HandlerExport` must have non-empty `capability` and `handler_name`.
    /// 4. Every `ImportDeclaration` must have a non-empty `source_package`.
    ///
    /// # Errors
    ///
    /// Returns the first `PackageValidationError` encountered.
    pub fn validate(&self) -> Result<(), PackageValidationError> {
        if self.trust_level == TrustLevel::Unsafe && self.unsafe_surface.is_empty() {
            return Err(PackageValidationError::UnsafeWithoutSurface);
        }
        for export in &self.exports {
            if export.name.is_empty() {
                return Err(PackageValidationError::ExportNameEmpty);
            }
        }
        for handler in &self.handlers {
            if handler.capability.is_empty() || handler.handler_name.is_empty() {
                return Err(PackageValidationError::HandlerFieldEmpty);
            }
        }
        for import in &self.imports {
            if import.source_package.is_empty() {
                return Err(PackageValidationError::ImportSourceEmpty);
            }
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
            // G12 fields — all empty / None for minimal test fixture
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
        for level in [
            TrustLevel::Verified,
            TrustLevel::Assumed,
            TrustLevel::Unverified,
        ] {
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

    // ── G12: full_manifest_cbor_round_trip ───────────────────────────────
    // Spec scenario: "PackageManifest with all G12 fields round-trips through CBOR"
    //   GIVEN a PackageDef with all G12 fields populated
    //   WHEN serialized to CBOR and deserialized
    //   THEN all fields are equal to the original
    #[test]
    fn full_manifest_cbor_round_trip() {
        use crate::export::{ExportDeclaration, ExportStability, ExportVisibility};
        use crate::handler::HandlerExport;
        use crate::import::ImportDeclaration;
        use crate::verification::PackageVerificationReport;

        let def = PackageDef {
            name: "payments.stripe".to_string(),
            version: "1.2.0".to_string(),
            trust_level: TrustLevel::Assumed,
            required_capabilities: vec!["http.call:Stripe".to_string()],
            exported_capabilities: vec!["payment.charge:PaymentProvider".to_string()],
            assumptions: vec![],
            unsafe_surface: vec![],
            artifact_hashes: vec![],
            build_env_hash: None,
            handlers: vec![HandlerExport {
                capability: "payment.charge:PaymentProvider".to_string(),
                handler_name: "StripePayment".to_string(),
                trust_level: TrustLevel::Assumed,
            }],
            contracts: vec!["idempotent_by_key".to_string()],
            exports: vec![ExportDeclaration {
                name: "charge".to_string(),
                signature: "PaymentRequest -> Result<PaymentReceipt, PaymentError>".to_string(),
                effects: vec!["payment.charge:PaymentProvider".to_string()],
                contracts: vec!["idempotent_by_key".to_string()],
                visibility: ExportVisibility::Public,
                stability: ExportStability::Stable,
            }],
            imports: vec![ImportDeclaration {
                source_package: "utils.core".to_string(),
                items: vec!["Result".to_string()],
                version_constraint: Some("^2.0".to_string()),
            }],
            boundaries: vec!["boundary.Stripe".to_string()],
            license: Some("Apache-2.0".to_string()),
            provenance: Some("https://ci.example.com/builds/42".to_string()),
            verification_report: Some(PackageVerificationReport {
                exports_verified: 1,
                effects_declared: 1,
                contracts_proven: 1,
            }),
            graph_schema: Some(3),
            core_ir_schema: Some(2),
        };

        let manifest = PackageManifest::from_def(def);

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&manifest, &mut buf).expect("CBOR serialization must succeed");

        let decoded: PackageManifest =
            ciborium::de::from_reader(buf.as_slice()).expect("CBOR deserialization must succeed");

        assert_eq!(
            decoded, manifest,
            "round-tripped manifest must equal original"
        );
    }

    // ── G12: g12_fields_change_hash ──────────────────────────────────────
    // Spec scenario: "G12 fields are included in the content hash"
    //   GIVEN two manifests differing only in graph_schema
    //   WHEN blake3_hex() is called on each
    //   THEN the hashes differ
    #[test]
    fn g12_fields_change_hash() {
        let mut def1 = minimal_def(TrustLevel::Verified);
        def1.graph_schema = Some(1);

        let mut def2 = minimal_def(TrustLevel::Verified);
        def2.graph_schema = Some(2);

        let m1 = PackageManifest::from_def(def1);
        let m2 = PackageManifest::from_def(def2);

        assert_ne!(
            m1.blake3_hex().unwrap(),
            m2.blake3_hex().unwrap(),
            "different graph_schema must produce different hashes"
        );
    }

    // ── G12: validate_rejects_export_with_empty_name ─────────────────────
    // Spec scenario: "ExportDeclaration with empty name is rejected"
    //   GIVEN a PackageDef with an export that has an empty name
    //   WHEN validate() is called
    //   THEN it returns Err(PackageValidationError::ExportNameEmpty)
    #[test]
    fn validate_rejects_export_with_empty_name() {
        use crate::export::{ExportDeclaration, ExportStability, ExportVisibility};

        let mut def = minimal_def(TrustLevel::Verified);
        def.exports.push(ExportDeclaration {
            name: "".to_string(), // deliberately empty
            signature: "() -> ()".to_string(),
            effects: vec![],
            contracts: vec![],
            visibility: ExportVisibility::Public,
            stability: ExportStability::Stable,
        });
        let m = PackageManifest::from_def(def);
        assert_eq!(m.validate(), Err(PackageValidationError::ExportNameEmpty));
    }

    // ── G12: validate_rejects_handler_with_empty_capability ──────────────
    // Spec scenario: "HandlerExport with empty capability is rejected"
    //   GIVEN a PackageDef with a handler export that has an empty capability
    //   WHEN validate() is called
    //   THEN it returns Err(PackageValidationError::HandlerFieldEmpty)
    #[test]
    fn validate_rejects_handler_with_empty_capability() {
        use crate::handler::HandlerExport;

        let mut def = minimal_def(TrustLevel::Verified);
        def.handlers.push(HandlerExport {
            capability: "".to_string(), // deliberately empty
            handler_name: "MyHandler".to_string(),
            trust_level: TrustLevel::Assumed,
        });
        let m = PackageManifest::from_def(def);
        assert_eq!(m.validate(), Err(PackageValidationError::HandlerFieldEmpty));
    }

    // ── G12: validate_rejects_handler_with_empty_name ────────────────────
    // TRIANGULATE: HandlerExport with empty handler_name is also rejected.
    #[test]
    fn validate_rejects_handler_with_empty_name() {
        use crate::handler::HandlerExport;

        let mut def = minimal_def(TrustLevel::Verified);
        def.handlers.push(HandlerExport {
            capability: "payment.charge".to_string(),
            handler_name: "".to_string(), // deliberately empty
            trust_level: TrustLevel::Assumed,
        });
        let m = PackageManifest::from_def(def);
        assert_eq!(m.validate(), Err(PackageValidationError::HandlerFieldEmpty));
    }

    // ── G12: validate_rejects_import_with_empty_source ───────────────────
    // Spec scenario: "ImportDeclaration with empty source_package is rejected"
    //   GIVEN a PackageDef with an import that has an empty source_package
    //   WHEN validate() is called
    //   THEN it returns Err(PackageValidationError::ImportSourceEmpty)
    #[test]
    fn validate_rejects_import_with_empty_source() {
        use crate::import::ImportDeclaration;

        let mut def = minimal_def(TrustLevel::Verified);
        def.imports.push(ImportDeclaration {
            source_package: "".to_string(), // deliberately empty
            items: vec![],
            version_constraint: None,
        });
        let m = PackageManifest::from_def(def);
        assert_eq!(m.validate(), Err(PackageValidationError::ImportSourceEmpty));
    }

    // ── G12: validate_accepts_valid_g12_fields ────────────────────────────
    // Spec scenario: "Valid G12 fields pass validation"
    //   GIVEN a PackageDef with all G12 fields populated correctly
    //   WHEN validate() is called
    //   THEN it returns Ok(())
    #[test]
    fn validate_accepts_valid_g12_fields() {
        use crate::export::{ExportDeclaration, ExportStability, ExportVisibility};
        use crate::handler::HandlerExport;
        use crate::import::ImportDeclaration;

        let mut def = minimal_def(TrustLevel::Verified);
        def.exports.push(ExportDeclaration {
            name: "charge".to_string(),
            signature: "Req -> Res".to_string(),
            effects: vec![],
            contracts: vec![],
            visibility: ExportVisibility::Public,
            stability: ExportStability::Stable,
        });
        def.handlers.push(HandlerExport {
            capability: "payment.charge".to_string(),
            handler_name: "StripePayment".to_string(),
            trust_level: TrustLevel::Verified,
        });
        def.imports.push(ImportDeclaration {
            source_package: "utils.core".to_string(),
            items: vec![],
            version_constraint: None,
        });
        def.boundaries = vec!["boundary.payments".to_string()];
        def.license = Some("MIT".to_string());
        def.graph_schema = Some(1);
        def.core_ir_schema = Some(1);

        let m = PackageManifest::from_def(def);
        assert_eq!(m.validate(), Ok(()));
    }

    // ── G12: minimal_def_new_fields_default_to_empty ─────────────────────
    // TRIANGULATE: a minimal def has empty G12 collections and None options.
    #[test]
    fn minimal_def_new_fields_default_to_empty() {
        let m = PackageManifest::from_def(minimal_def(TrustLevel::Verified));
        assert!(m.handlers.is_empty());
        assert!(m.contracts.is_empty());
        assert!(m.exports.is_empty());
        assert!(m.imports.is_empty());
        assert!(m.boundaries.is_empty());
        assert!(m.license.is_none());
        assert!(m.provenance.is_none());
        assert!(m.verification_report.is_none());
        assert!(m.graph_schema.is_none());
        assert!(m.core_ir_schema.is_none());
    }
}
