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
use std::collections::BTreeMap;

use crate::assumption::PackageAssumption;
use crate::export::ExportDeclaration;
use crate::handler::HandlerExport;
use crate::import::ImportDeclaration;
use crate::surface::UnsafeSurfaceEntry;
use crate::trust::TrustLevel;
use crate::verification::PackageVerificationReport;

// ── ReproducibleBuildEvidence ─────────────────────────────────────────────

/// Locally-recorded reproducible-build evidence for a package.
///
/// This is metadata recorded at publish time to enable local trust verification.
/// It does **not** prove the build is reproducible — no rebuild is executed and
/// no remote attestation is consulted.  It records the inputs and recipe hash
/// so that reviewers can reason about build determinism locally.
///
/// # Note on scope
///
/// This is LOCAL evidence metadata only.  It cannot guarantee remote
/// reproducibility, transparency-log attestation, or Sigstore signatures.
///
/// # Hash format
///
/// All `*_hash` / `*_digest` fields must be 64 lower-case hex characters
/// (a BLAKE3 hex digest).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReproducibleBuildEvidence {
    /// BLAKE3 hex digest of the combined build inputs.
    ///
    /// Canonical formula: `BLAKE3(source_digest_utf8_bytes || toolchain_id_utf8_bytes)`.
    /// Use [`ReproducibleBuildEvidence::compute_build_inputs_hash`] to derive this value.
    /// 64 lower-case hex characters.
    pub build_inputs_hash: String,
    /// Identifier of the toolchain used to build this package.
    ///
    /// Examples: `"rustc-1.77.0-stable-x86_64-unknown-linux-gnu"`,
    /// `"ail-toolchain-0.3.0"`.
    pub toolchain_id: String,
    /// BLAKE3 hex digest of the source archive used as build input.
    ///
    /// 64 lower-case hex characters.
    pub source_digest: String,
    /// BLAKE3 hex digest of the deterministic build recipe.
    ///
    /// Covers build flags, lock-file content, and build-script identity.
    /// 64 lower-case hex characters.
    pub recipe_hash: String,
}

impl ReproducibleBuildEvidence {
    /// Construct a new `ReproducibleBuildEvidence`, deriving `build_inputs_hash`
    /// from `source_digest` and `toolchain_id`.
    ///
    /// The caller is responsible for supplying a valid BLAKE3 hex string for
    /// `source_digest` and `recipe_hash` (64 lower-case hex characters).
    pub fn new(
        source_digest: impl Into<String>,
        toolchain_id: impl Into<String>,
        recipe_hash: impl Into<String>,
    ) -> Self {
        let source_digest = source_digest.into();
        let toolchain_id = toolchain_id.into();
        let recipe_hash = recipe_hash.into();
        let build_inputs_hash = Self::compute_build_inputs_hash(&source_digest, &toolchain_id);
        Self {
            build_inputs_hash,
            toolchain_id,
            source_digest,
            recipe_hash,
        }
    }

    /// Derive the `build_inputs_hash` from `source_digest` and `toolchain_id`.
    ///
    /// Formula: `BLAKE3(source_digest_utf8_bytes || toolchain_id_utf8_bytes)`.
    /// Both inputs are treated as raw UTF-8 bytes (the hex string, not decoded).
    pub fn compute_build_inputs_hash(source_digest: &str, toolchain_id: &str) -> String {
        let mut hasher = Hasher::new();
        hasher.update(source_digest.as_bytes());
        hasher.update(toolchain_id.as_bytes());
        hasher.finalize().to_hex().to_string()
    }

    /// Compute the BLAKE3 content hash of this evidence record as a hex-encoded string.
    ///
    /// The hash covers the canonical CBOR serialization of the full record.
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
}

// ── Provenance ────────────────────────────────────────────────────────────

/// Structured provenance information linking a package to its upstream source
/// and build pipeline.
///
/// Replaces the original `Option<String>` provenance field with typed metadata
/// so callers can distinguish CI build URLs from source repository references.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Provenance {
    /// CI run URL or source archive URI (e.g. `"https://ci.example.com/builds/42"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Source repository URL (e.g. `"https://github.com/org/repo"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_repository: Option<String>,
    /// Git commit hash or source tree hash at build time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_hash: Option<String>,
    /// CI build identifier (e.g. `"build-42"`, `"run-abc123"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_id: Option<String>,
}

impl Provenance {
    /// Construct a `Provenance` with only a URL set.
    ///
    /// Convenience constructor for the common case of a single CI/archive URL.
    pub fn from_url(url: impl Into<String>) -> Self {
        Provenance {
            url: Some(url.into()),
            ..Default::default()
        }
    }
}

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
    /// Structured provenance linking to the upstream source and build pipeline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
    /// Verification report summarizing the results of the package review.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_report: Option<PackageVerificationReport>,
    /// Graph schema version this package was compiled against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_schema: Option<u32>,
    /// Core IR schema version this package was compiled against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub core_ir_schema: Option<u32>,

    // ── 4G fields ────────────────────────────────────────────────────────
    /// Locally-recorded reproducible-build evidence.
    ///
    /// Required for `TrustLevel::Verified` by
    /// [`crate::verification::validate_verified_package_evidence`].
    /// Optional for all other trust tiers.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reproducible_evidence: Option<ReproducibleBuildEvidence>,
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
    /// A package declares a persisted WASM artifact without the matching ABI
    /// descriptor artifact.
    ///
    /// WASM packages must carry the ABI descriptor that runtimes and package
    /// compatibility gates use to validate invocation contracts.
    WasmArtifactMissingAbiDescriptor,
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
            PackageValidationError::WasmArtifactMissingAbiDescriptor => write!(
                f,
                "a wasm artifact declaration is missing its wasm-abi-descriptor artifact"
            ),
        }
    }
}

impl std::error::Error for PackageValidationError {}

// ── PackageManifestIssue ─────────────────────────────────────────────────

/// Stable, redacted validation diagnostic produced for production publishing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageManifestIssue {
    /// Machine-readable issue class.
    pub kind: PackageManifestIssueKind,
    /// Redacted location descriptor. Never includes user-provided manifest values.
    pub descriptor: PackageManifestIssueDescriptor,
    /// Human-readable diagnostic. Never includes user-provided manifest values.
    pub message: String,
}

/// Machine-readable production manifest diagnostic class.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PackageManifestIssueKind {
    InvalidPackageName,
    InvalidVersion,
    UnsafeWithoutSurface,
    ExportNameEmpty,
    HandlerFieldEmpty,
    ImportSourceEmpty,
    MissingLicense,
    MissingEntryMetadata,
    MissingAbiDescriptorArtifact,
    DuplicateDependency,
    DuplicateCapability,
    DuplicateExport,
}

/// Redacted manifest location for a production validation issue.
///
/// `path` is a stable manifest field path such as `manifest.exports.name`.
/// `index` identifies the offending collection entry when applicable.
/// `duplicate_of` identifies the first matching entry for duplicate issues.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageManifestIssueDescriptor {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate_of: Option<usize>,
}

impl PackageManifestIssue {
    fn new(
        kind: PackageManifestIssueKind,
        path: &'static str,
        index: Option<usize>,
        duplicate_of: Option<usize>,
        message: &'static str,
    ) -> Self {
        Self {
            kind,
            descriptor: PackageManifestIssueDescriptor {
                path: path.to_string(),
                index,
                duplicate_of,
            },
            message: message.to_string(),
        }
    }
}

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
    /// Structured provenance linking to the upstream source and build pipeline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
    /// Verification report summarizing the results of the package review.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_report: Option<PackageVerificationReport>,
    /// Graph schema version this package was compiled against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph_schema: Option<u32>,
    /// Core IR schema version this package was compiled against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub core_ir_schema: Option<u32>,

    // ── 4G fields ────────────────────────────────────────────────────────
    /// Locally-recorded reproducible-build evidence.
    ///
    /// Required for `TrustLevel::Verified` by
    /// [`crate::verification::validate_verified_package_evidence`].
    /// Optional for all other trust tiers.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reproducible_evidence: Option<ReproducibleBuildEvidence>,
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
            // 4G fields
            reproducible_evidence: def.reproducible_evidence,
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
        if has_artifact_role(&self.artifact_hashes, "wasm-artifact")
            && !has_artifact_role(&self.artifact_hashes, "wasm-abi-descriptor")
        {
            return Err(PackageValidationError::WasmArtifactMissingAbiDescriptor);
        }
        Ok(())
    }

    /// Return all production-publish validation diagnostics in deterministic order.
    ///
    /// Diagnostics use stable redacted descriptors: issue paths and indexes are
    /// reported, but manifest values are never copied into the descriptor or
    /// message. This keeps local paths, bearer tokens, and other accidental
    /// secrets out of package workflow errors.
    pub fn production_validation_issues(&self) -> Vec<PackageManifestIssue> {
        let mut issues = Vec::new();

        if !is_valid_package_name(&self.name) {
            issues.push(PackageManifestIssue::new(
                PackageManifestIssueKind::InvalidPackageName,
                "manifest.name",
                None,
                None,
                "package name must be non-empty lowercase package coordinates",
            ));
        }

        if semver::Version::parse(&self.version).is_err() {
            issues.push(PackageManifestIssue::new(
                PackageManifestIssueKind::InvalidVersion,
                "manifest.version",
                None,
                None,
                "package version must be valid semantic version metadata",
            ));
        }

        if self.trust_level == TrustLevel::Unsafe && self.unsafe_surface.is_empty() {
            issues.push(PackageManifestIssue::new(
                PackageManifestIssueKind::UnsafeWithoutSurface,
                "manifest.unsafe_surface",
                None,
                None,
                "unsafe packages must declare at least one unsafe surface entry",
            ));
        }

        if self
            .license
            .as_ref()
            .map_or(true, |license| license.trim().is_empty())
        {
            issues.push(PackageManifestIssue::new(
                PackageManifestIssueKind::MissingLicense,
                "manifest.license",
                None,
                None,
                "production package manifests must declare license metadata",
            ));
        }

        push_duplicate_string_issues(
            &mut issues,
            &self.required_capabilities,
            PackageManifestIssueKind::DuplicateCapability,
            "manifest.required_capabilities",
            "required capability entries must be unique",
        );
        push_duplicate_string_issues(
            &mut issues,
            &self.exported_capabilities,
            PackageManifestIssueKind::DuplicateCapability,
            "manifest.exported_capabilities",
            "exported capability entries must be unique",
        );
        push_duplicate_import_issues(&mut issues, &self.imports);
        if has_artifact_role(&self.artifact_hashes, "wasm-artifact")
            && !has_artifact_role(&self.artifact_hashes, "wasm-abi-descriptor")
        {
            issues.push(PackageManifestIssue::new(
                PackageManifestIssueKind::MissingAbiDescriptorArtifact,
                "manifest.artifact_hashes",
                None,
                None,
                "wasm packages must declare the ABI descriptor artifact",
            ));
        }

        for (index, export) in self.exports.iter().enumerate() {
            if export.name.trim().is_empty() {
                issues.push(PackageManifestIssue::new(
                    PackageManifestIssueKind::ExportNameEmpty,
                    "manifest.exports.name",
                    Some(index),
                    None,
                    "export declarations must include an export name",
                ));
            }
            if export.signature.trim().is_empty() {
                issues.push(PackageManifestIssue::new(
                    PackageManifestIssueKind::MissingEntryMetadata,
                    "manifest.exports.signature",
                    Some(index),
                    None,
                    "export declarations must include entry signature metadata",
                ));
            }
        }
        push_duplicate_export_issues(&mut issues, &self.exports);

        for (index, handler) in self.handlers.iter().enumerate() {
            if handler.capability.trim().is_empty() || handler.handler_name.trim().is_empty() {
                issues.push(PackageManifestIssue::new(
                    PackageManifestIssueKind::HandlerFieldEmpty,
                    "manifest.handlers",
                    Some(index),
                    None,
                    "handler exports must include capability and handler metadata",
                ));
            }
        }

        for (index, import) in self.imports.iter().enumerate() {
            if import.source_package.trim().is_empty() {
                issues.push(PackageManifestIssue::new(
                    PackageManifestIssueKind::ImportSourceEmpty,
                    "manifest.imports.source_package",
                    Some(index),
                    None,
                    "import declarations must include source package metadata",
                ));
            }
        }

        issues
    }

    /// Validate a manifest for the production package publishing workflow.
    ///
    /// Unlike [`PackageManifest::validate`], this returns every publish-facing
    /// diagnostic instead of only the first structural invariant failure.
    pub fn validate_for_publish(&self) -> Result<(), Vec<PackageManifestIssue>> {
        let issues = self.production_validation_issues();
        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues)
        }
    }
}

fn is_valid_package_name(name: &str) -> bool {
    if name.is_empty() || name.starts_with('.') || name.ends_with('.') || name.contains("..") {
        return false;
    }

    name.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
    })
}

fn has_artifact_role(artifact_hashes: &[ArtifactHashEntry], role: &str) -> bool {
    artifact_hashes
        .iter()
        .any(|entry| entry.role.trim() == role)
}

fn push_duplicate_string_issues(
    issues: &mut Vec<PackageManifestIssue>,
    values: &[String],
    kind: PackageManifestIssueKind,
    path: &'static str,
    message: &'static str,
) {
    let mut first_indexes: BTreeMap<&str, usize> = BTreeMap::new();
    let mut duplicates = Vec::new();

    for (index, value) in values.iter().enumerate() {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }

        if let Some(first_index) = first_indexes.get(value) {
            duplicates.push((index, *first_index));
        } else {
            first_indexes.insert(value, index);
        }
    }

    duplicates.sort_by_key(|(index, first_index)| (*index, *first_index));
    for (index, first_index) in duplicates {
        issues.push(PackageManifestIssue::new(
            kind.clone(),
            path,
            Some(index),
            Some(first_index),
            message,
        ));
    }
}

fn push_duplicate_import_issues(
    issues: &mut Vec<PackageManifestIssue>,
    imports: &[ImportDeclaration],
) {
    let mut first_indexes: BTreeMap<&str, usize> = BTreeMap::new();
    let mut duplicates = Vec::new();

    for (index, import) in imports.iter().enumerate() {
        let source_package = import.source_package.trim();
        if source_package.is_empty() {
            continue;
        }

        if let Some(first_index) = first_indexes.get(source_package) {
            duplicates.push((index, *first_index));
        } else {
            first_indexes.insert(source_package, index);
        }
    }

    duplicates.sort_by_key(|(index, first_index)| (*index, *first_index));
    for (index, first_index) in duplicates {
        issues.push(PackageManifestIssue::new(
            PackageManifestIssueKind::DuplicateDependency,
            "manifest.imports.source_package",
            Some(index),
            Some(first_index),
            "dependency imports must be unique by source package",
        ));
    }
}

fn push_duplicate_export_issues(
    issues: &mut Vec<PackageManifestIssue>,
    exports: &[ExportDeclaration],
) {
    let mut first_indexes: BTreeMap<&str, usize> = BTreeMap::new();
    let mut duplicates = Vec::new();

    for (index, export) in exports.iter().enumerate() {
        let name = export.name.trim();
        if name.is_empty() {
            continue;
        }

        if let Some(first_index) = first_indexes.get(name) {
            duplicates.push((index, *first_index));
        } else {
            first_indexes.insert(name, index);
        }
    }

    duplicates.sort_by_key(|(index, first_index)| (*index, *first_index));
    for (index, first_index) in duplicates {
        issues.push(PackageManifestIssue::new(
            PackageManifestIssueKind::DuplicateExport,
            "manifest.exports.name",
            Some(index),
            Some(first_index),
            "export declarations must be unique by export name",
        ));
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
            // 4G fields
            reproducible_evidence: None,
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

    #[test]
    fn validate_rejects_wasm_artifact_without_abi_descriptor() {
        let mut def = minimal_def(TrustLevel::Verified);
        def.artifact_hashes.push(ArtifactHashEntry {
            role: "wasm-artifact".to_string(),
            hash: "a".repeat(64),
        });

        let manifest = PackageManifest::from_def(def);

        assert_eq!(
            manifest.validate(),
            Err(PackageValidationError::WasmArtifactMissingAbiDescriptor)
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
                trust_state: None,
            }],
            imports: vec![ImportDeclaration {
                source_package: "utils.core".to_string(),
                items: vec!["Result".to_string()],
                version_constraint: Some("^2.0".to_string()),
            }],
            boundaries: vec!["boundary.Stripe".to_string()],
            license: Some("Apache-2.0".to_string()),
            provenance: Some(Provenance::from_url("https://ci.example.com/builds/42")),
            verification_report: Some(PackageVerificationReport {
                package: "payments.stripe".to_string(),
                version: "1.2.0".to_string(),
                exports_verified: vec!["charge".to_string()],
                effects_declared: vec!["payment.charge:PaymentProvider".to_string()],
                assumptions: vec![],
                unsafe_surface: vec![],
                artifact_hashes: vec![],
            }),
            graph_schema: Some(3),
            core_ir_schema: Some(2),
            // 4G fields
            reproducible_evidence: None,
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
            trust_state: None,
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
            trust_state: None,
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
        assert!(m.reproducible_evidence.is_none());
    }

    // ── 4G: reproducible_evidence_cbor_round_trip ─────────────────────────
    // Spec scenario: "ReproducibleBuildEvidence round-trips through CBOR"
    //   GIVEN a ReproducibleBuildEvidence constructed with new()
    //   WHEN serialized to CBOR and deserialized
    //   THEN all fields are equal to the original
    #[test]
    fn reproducible_evidence_cbor_round_trip() {
        let source_digest = "b".repeat(64);
        let evidence =
            ReproducibleBuildEvidence::new(source_digest.clone(), "rustc-1.77.0", "c".repeat(64));
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&evidence, &mut buf).expect("CBOR encode must succeed");
        let decoded: ReproducibleBuildEvidence =
            ciborium::de::from_reader(buf.as_slice()).expect("CBOR decode must succeed");
        assert_eq!(decoded, evidence);
    }

    // ── 4G: reproducible_evidence_blake3_hex_is_deterministic ─────────────
    // Spec scenario: "Evidence hash is deterministic"
    //   GIVEN two identical ReproducibleBuildEvidence values
    //   WHEN blake3_hex() is called on each
    //   THEN both return the same hex string
    #[test]
    fn reproducible_evidence_blake3_hex_is_deterministic() {
        let e1 = ReproducibleBuildEvidence::new("a".repeat(64), "toolchain-1", "d".repeat(64));
        let e2 = ReproducibleBuildEvidence::new("a".repeat(64), "toolchain-1", "d".repeat(64));
        let h1 = e1.blake3_hex().expect("hash must succeed");
        let h2 = e2.blake3_hex().expect("hash must succeed");
        assert_eq!(h1.len(), 64, "evidence hash must be 64 chars");
        assert_eq!(h1, h2, "identical evidence must hash to same value");
    }

    // ── 4G: compute_build_inputs_hash_is_deterministic ────────────────────
    // TRIANGULATE: compute_build_inputs_hash is stable.
    #[test]
    fn compute_build_inputs_hash_is_deterministic() {
        let h1 = ReproducibleBuildEvidence::compute_build_inputs_hash("src", "tc");
        let h2 = ReproducibleBuildEvidence::compute_build_inputs_hash("src", "tc");
        assert_eq!(h1.len(), 64);
        assert_eq!(h1, h2);
    }

    // ── 4G: compute_build_inputs_hash_differs_for_different_inputs ─────────
    // TRIANGULATE: different inputs produce different build_inputs_hash.
    #[test]
    fn compute_build_inputs_hash_differs_for_different_inputs() {
        let h1 = ReproducibleBuildEvidence::compute_build_inputs_hash("src-a", "tc-1");
        let h2 = ReproducibleBuildEvidence::compute_build_inputs_hash("src-b", "tc-1");
        assert_ne!(
            h1, h2,
            "different source_digest must produce different hash"
        );
    }

    // ── 4G: reproducible_evidence_changes_manifest_hash ───────────────────
    // Spec scenario: "ReproducibleBuildEvidence is included in manifest content hash"
    //   GIVEN two manifests differing only in reproducible_evidence
    //   WHEN blake3_hex() is called
    //   THEN the hashes differ
    #[test]
    fn reproducible_evidence_changes_manifest_hash() {
        let mut def1 = minimal_def(TrustLevel::Verified);
        def1.reproducible_evidence = Some(ReproducibleBuildEvidence::new(
            "a".repeat(64),
            "tc-1",
            "b".repeat(64),
        ));
        let mut def2 = minimal_def(TrustLevel::Verified);
        def2.reproducible_evidence = None;
        let m1 = PackageManifest::from_def(def1);
        let m2 = PackageManifest::from_def(def2);
        assert_ne!(
            m1.blake3_hex().unwrap(),
            m2.blake3_hex().unwrap(),
            "reproducible_evidence presence must change manifest hash"
        );
    }

    // ── production diagnostics ───────────────────────────────────────────
    // Spec scenario: "Production package manifest diagnostics are stable"
    //   GIVEN a manifest with invalid metadata and duplicate declarations
    //   WHEN production_validation_issues() is called
    //   THEN every issue is returned in deterministic order with redacted descriptors
    #[test]
    fn production_validation_issues_are_stable_and_redacted() {
        use crate::export::{ExportDeclaration, ExportStability, ExportVisibility};
        use crate::import::ImportDeclaration;

        let secret_path = "/tmp/private/token-abc123";
        let mut def = minimal_def(TrustLevel::Verified);
        def.name = secret_path.to_string();
        def.version = "not-semver".to_string();
        def.required_capabilities = vec!["pay.charge".to_string(), "pay.charge".to_string()];
        def.exported_capabilities = vec!["pay.refund".to_string(), "pay.refund".to_string()];
        def.exports = vec![
            ExportDeclaration {
                name: "charge".to_string(),
                signature: "".to_string(),
                effects: vec![],
                contracts: vec![],
                visibility: ExportVisibility::Public,
                stability: ExportStability::Stable,
                trust_state: None,
            },
            ExportDeclaration {
                name: "charge".to_string(),
                signature: "Req -> Res".to_string(),
                effects: vec![],
                contracts: vec![],
                visibility: ExportVisibility::Public,
                stability: ExportStability::Stable,
                trust_state: None,
            },
        ];
        def.imports = vec![
            ImportDeclaration {
                source_package: "utils.core".to_string(),
                items: vec![],
                version_constraint: None,
            },
            ImportDeclaration {
                source_package: "utils.core".to_string(),
                items: vec![],
                version_constraint: Some("^1.0".to_string()),
            },
        ];

        let manifest = PackageManifest::from_def(def);
        let issues = manifest.production_validation_issues();
        let kinds: Vec<_> = issues.iter().map(|issue| issue.kind.clone()).collect();

        assert_eq!(
            kinds,
            vec![
                PackageManifestIssueKind::InvalidPackageName,
                PackageManifestIssueKind::InvalidVersion,
                PackageManifestIssueKind::MissingLicense,
                PackageManifestIssueKind::DuplicateCapability,
                PackageManifestIssueKind::DuplicateCapability,
                PackageManifestIssueKind::DuplicateDependency,
                PackageManifestIssueKind::MissingEntryMetadata,
                PackageManifestIssueKind::DuplicateExport,
            ],
            "production diagnostics must have a stable order"
        );

        assert_eq!(issues[0].descriptor.path, "manifest.name");
        assert_eq!(issues[3].descriptor.index, Some(1));
        assert_eq!(issues[3].descriptor.duplicate_of, Some(0));
        assert_eq!(issues[5].descriptor.path, "manifest.imports.source_package");
        assert_eq!(issues[6].descriptor.path, "manifest.exports.signature");

        let encoded = serde_json::to_string(&issues).expect("issues must serialize");
        assert!(
            !encoded.contains(secret_path),
            "diagnostics must not leak local paths or token-shaped values"
        );
        assert!(
            !encoded.contains("pay.charge") && !encoded.contains("utils.core"),
            "diagnostics must not leak manifest values"
        );
    }

    #[test]
    fn production_validation_reports_wasm_artifact_without_abi_descriptor() {
        use crate::export::{ExportDeclaration, ExportStability, ExportVisibility};

        let mut def = minimal_def(TrustLevel::Verified);
        def.license = Some("Apache-2.0".to_string());
        def.exports.push(ExportDeclaration {
            name: "main".to_string(),
            signature: "() -> Int".to_string(),
            effects: vec![],
            contracts: vec![],
            visibility: ExportVisibility::Public,
            stability: ExportStability::Stable,
            trust_state: None,
        });
        def.artifact_hashes.push(ArtifactHashEntry {
            role: "wasm-artifact".to_string(),
            hash: "a".repeat(64),
        });

        let manifest = PackageManifest::from_def(def);
        let issues = manifest.production_validation_issues();

        assert_eq!(issues.len(), 1);
        assert_eq!(
            issues[0].kind,
            PackageManifestIssueKind::MissingAbiDescriptorArtifact
        );
        assert_eq!(issues[0].descriptor.path, "manifest.artifact_hashes");
    }

    // ── production diagnostics accepts publish-ready manifest ─────────────
    // TRIANGULATE: publish diagnostics accept valid package metadata.
    #[test]
    fn validate_for_publish_accepts_manifest_with_required_metadata() {
        use crate::export::{ExportDeclaration, ExportStability, ExportVisibility};

        let mut def = minimal_def(TrustLevel::Verified);
        def.license = Some("Apache-2.0".to_string());
        def.exports.push(ExportDeclaration {
            name: "charge".to_string(),
            signature: "Req -> Res".to_string(),
            effects: vec![],
            contracts: vec![],
            visibility: ExportVisibility::Public,
            stability: ExportStability::Stable,
            trust_state: None,
        });
        def.artifact_hashes.push(ArtifactHashEntry {
            role: "wasm-artifact".to_string(),
            hash: "a".repeat(64),
        });
        def.artifact_hashes.push(ArtifactHashEntry {
            role: "wasm-abi-descriptor".to_string(),
            hash: "b".repeat(64),
        });

        let manifest = PackageManifest::from_def(def);

        assert_eq!(manifest.validate_for_publish(), Ok(()));
    }
}
