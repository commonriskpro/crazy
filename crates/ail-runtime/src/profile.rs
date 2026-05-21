// ── ail-runtime::profile ─────────────────────────────────────────────────
//
// Runtime profile model: deny-by-default capability configuration.
//
// `RuntimeProfile` carries the hashes and grants that drive preflight.
// Construction is via `RuntimeProfile::new`; all fields are read-only after
// construction.
//
// Phase 12 adds `min_package_trust: Option<TrustLevel>` as an optional
// builder field (`with_package_trust`) so existing `RuntimeProfile::new`
// call sites are unaffected.

use ail_package::trust::TrustLevel;

// ── CapabilityId ─────────────────────────────────────────────────────────

/// A string-keyed capability identifier.
///
/// Using strings rather than a hard-coded enum keeps the capability space
/// open for extension without changing the runtime crate.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CapabilityId(String);

impl CapabilityId {
    /// Create a new `CapabilityId` from any string-like value.
    pub fn new(name: impl Into<String>) -> Self {
        CapabilityId(name.into())
    }

    /// Return the underlying capability name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CapabilityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ── CapabilityGrant ──────────────────────────────────────────────────────

/// An explicit grant that allows `module` to use `capability`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityGrant {
    /// The WASM module name this grant applies to.
    pub module: String,
    /// The capability being granted.
    pub capability: CapabilityId,
}

// ── ResourceLimits ───────────────────────────────────────────────────────

/// Optional resource constraints enforced during instantiation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceLimits {
    /// Maximum linear memory in bytes, if constrained.
    pub max_memory_bytes: Option<u64>,
    /// Maximum Wasmtime fuel units, if constrained.
    pub max_fuel: Option<u64>,
}

// ── RuntimeProfile ───────────────────────────────────────────────────────

/// Immutable runtime configuration for one WASM module.
///
/// A profile records:
/// - **Hashes**: BLAKE3 digests of the WASM binary, verification report,
///   and capability manifest.  Preflight rejects any binary whose hash
///   does not match `module_hash`.
/// - **Grants**: explicit capability grants.  Absence is denial.
/// - **Limits**: optional resource caps applied at instantiation time.
#[derive(Clone, Debug)]
pub struct RuntimeProfile {
    name: String,
    module_hash: String,
    verification_report_hash: String,
    capability_manifest_hash: String,
    grants: Vec<CapabilityGrant>,
    limits: ResourceLimits,
    /// Optional minimum trust tier for package manifests.
    ///
    /// `None` disables the package trust gate entirely (preserves existing
    /// behaviour for callers that don't opt in to package trust checking).
    /// `Some(level)` causes preflight to reject any package manifest whose
    /// `trust_level` does not satisfy `level`.
    min_package_trust: Option<TrustLevel>,

    /// Whether preflight step 5 must verify that every granted capability
    /// has a registered handler.
    ///
    /// Defaults to `false` for backward compatibility.  Set to `true` via
    /// [`with_handler_binding_required`] to enforce handler binding at
    /// instantiation time.
    ///
    /// [`with_handler_binding_required`]: RuntimeProfile::with_handler_binding_required
    require_handler_binding: bool,
}

impl RuntimeProfile {
    /// Construct a `RuntimeProfile` from its constituent parts.
    ///
    /// Hash strings are hex-encoded BLAKE3 digests (64 lower-case characters).
    /// No validation of the hex encoding is performed here; callers must
    /// ensure correctness.
    pub fn new(
        name: String,
        module_hash: String,
        verification_report_hash: String,
        capability_manifest_hash: String,
        grants: Vec<CapabilityGrant>,
        limits: ResourceLimits,
    ) -> Self {
        RuntimeProfile {
            name,
            module_hash,
            verification_report_hash,
            capability_manifest_hash,
            grants,
            limits,
            min_package_trust: None,
            require_handler_binding: false,
        }
    }

    /// Set the minimum package trust tier for this profile.
    ///
    /// Consumes `self` and returns a new `RuntimeProfile` with
    /// `min_package_trust` set to `Some(level)`.  Use this builder method
    /// to opt in to package trust gating without changing the `new`
    /// constructor signature.
    pub fn with_package_trust(mut self, level: TrustLevel) -> Self {
        self.min_package_trust = Some(level);
        self
    }

    /// Require that every granted capability has a bound handler at preflight.
    ///
    /// When this is set, `validate_and_instantiate` will fail with
    /// [`PreflightFailure::HandlerNotBound`](crate::error::PreflightFailure::HandlerNotBound)
    /// if any granted capability lacks a registered handler.
    ///
    /// The default is `false` — existing call sites are unaffected.
    pub fn with_handler_binding_required(mut self) -> Self {
        self.require_handler_binding = true;
        self
    }

    /// Profile name (human-readable label).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// BLAKE3 hex digest of the expected WASM binary.
    pub fn module_hash(&self) -> &str {
        &self.module_hash
    }

    /// BLAKE3 hex digest of the verification report used to produce this profile.
    pub fn verification_report_hash(&self) -> &str {
        &self.verification_report_hash
    }

    /// BLAKE3 hex digest of the canonical CBOR capability manifest.
    pub fn capability_manifest_hash(&self) -> &str {
        &self.capability_manifest_hash
    }

    /// Ordered list of explicit capability grants.
    ///
    /// Any capability absent from this list is **denied** by default.
    pub fn grants(&self) -> &[CapabilityGrant] {
        &self.grants
    }

    /// Optional resource constraints.
    pub fn limits(&self) -> &ResourceLimits {
        &self.limits
    }

    /// Minimum package trust tier required by this profile.
    ///
    /// `None` means the package trust gate is disabled.
    pub fn min_package_trust(&self) -> Option<TrustLevel> {
        self.min_package_trust
    }

    /// Return `true` if `capability` is present in the grants list.
    ///
    /// Performs a linear scan; the grants list is expected to be small
    /// (single-digit items in practice).
    pub fn grants_capability(&self, capability: &CapabilityId) -> bool {
        self.grants.iter().any(|g| &g.capability == capability)
    }

    /// `true` if preflight must verify handler binding for all grants.
    pub fn require_handler_binding(&self) -> bool {
        self.require_handler_binding
    }
}
