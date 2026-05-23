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

use std::ops::Deref;
use std::sync::{Arc, RwLock, RwLockReadGuard};

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

// ── RateLimit ────────────────────────────────────────────────────────────

/// A per-capability or global rate limit.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct RateLimit {
    /// Capability this limit applies to.
    /// `None` means the limit applies globally to all capabilities.
    pub capability: Option<String>,
    /// Maximum calls per second allowed.
    pub max_calls_per_second: u64,
}

// ── ResourceLimits ───────────────────────────────────────────────────────

/// Optional resource constraints enforced during instantiation.
///
/// All fields are `Option<T>` — `None` means "no limit" for that axis.
/// Corresponds to the limits listed in `docs/runtime.md §Limits and sandboxing`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ResourceLimits {
    /// Maximum linear memory in bytes, if constrained.
    pub max_memory_bytes: Option<u64>,
    /// Maximum Wasmtime fuel units, if constrained.
    pub max_fuel: Option<u64>,
    /// Maximum wall-clock execution time.
    ///
    /// When exceeded the runtime raises `HostError::LimitExceeded`.
    pub timeout: Option<std::time::Duration>,
    /// Maximum number of capability calls per invocation.
    pub max_capability_calls: Option<u64>,
    /// Per-capability or global rate limits (calls per second).
    pub rate_limits: Option<Vec<RateLimit>>,
    /// Maximum payload size in bytes for a single capability call.
    pub payload_size_limit: Option<u64>,
    /// Maximum number of concurrent in-flight capability calls.
    pub concurrency_limit: Option<u64>,
    /// Maximum recursion depth / WASM call-stack frames.
    pub recursion_stack_limit: Option<u64>,
    /// Maximum size of a capability response payload in bytes.
    pub output_size_limit: Option<u64>,
}

// ── CapabilityState ──────────────────────────────────────────────────────

/// Lifecycle state of a single capability instance.
///
/// Corresponds to `docs/runtime.md §Capability lifecycle`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapabilityState {
    /// Capability has been declared in the module manifest.
    Declared,
    /// Capability has been verified against a verification report.
    Verified,
    /// Capability has been bound to a handler in the active profile.
    Bound,
    /// Capability is currently active and may be invoked.
    Active,
    /// Capability has been explicitly revoked; new calls are denied.
    Revoked,
    /// Capability grant has expired.
    Expired,
    /// Capability was denied by policy (never granted).
    Denied,
}

// ── InFlightPolicy ───────────────────────────────────────────────────────

/// Policy applied to in-flight capability calls when revocation occurs.
///
/// Corresponds to `docs/runtime.md §Capability lifecycle — Revocation`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InFlightPolicy {
    /// Allow currently executing calls to complete normally.
    AllowComplete,
    /// Cancel in-flight calls immediately.
    Cancel,
    /// Wait for a timeout, then cancel any still-running calls.
    TimeoutThenCancel,
}

// ── RevocationRecord ─────────────────────────────────────────────────────

/// A record of a capability revocation event.
///
/// Stored in the [`CapabilityRevocationRegistry`] so the runtime can
/// enforce denials after a `revoke` command has been issued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevocationRecord {
    /// WASM module whose grant is revoked.
    pub module: String,
    /// The capability being revoked.
    pub capability: String,
    /// Profile in which the revocation takes effect.
    pub profile: String,
    /// How to handle calls that are already in flight at revocation time.
    pub in_flight_policy: InFlightPolicy,
}

// ── CapabilityRevocationRegistry ────────────────────────────────────────

/// Runtime-mutable registry of capability revocations.
///
/// Unlike [`RuntimeProfile`] (which is immutable after construction),
/// revocations can be issued at runtime. The registry is consulted by the
/// host before dispatching any capability call.
#[derive(Clone, Debug, Default)]
pub struct CapabilityRevocationRegistry {
    records: Arc<RwLock<Vec<RevocationRecord>>>,
}

/// Read-only borrowed view of revocation records.
///
/// The registry is shared across cloned hosts/instances, so the slice must stay
/// protected by the read lock while borrowed. Use
/// [`CapabilityRevocationRegistry::records_snapshot`] when an owned `Vec` is
/// needed beyond this guard's lifetime.
pub struct RevocationRecords<'a>(RwLockReadGuard<'a, Vec<RevocationRecord>>);

impl Deref for RevocationRecords<'_> {
    type Target = [RevocationRecord];

    fn deref(&self) -> &Self::Target {
        self.0.as_slice()
    }
}

impl std::fmt::Debug for RevocationRecords<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.deref().fmt(f)
    }
}

impl CapabilityRevocationRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        CapabilityRevocationRegistry {
            records: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Revoke a capability grant for `module` in `profile`.
    pub fn revoke(
        &mut self,
        module: impl Into<String>,
        capability: impl Into<String>,
        profile: impl Into<String>,
        in_flight_policy: InFlightPolicy,
    ) {
        self.records
            .write()
            .expect("revocation registry lock must not be poisoned")
            .push(RevocationRecord {
                module: module.into(),
                capability: capability.into(),
                profile: profile.into(),
                in_flight_policy,
            });
    }

    /// Return `true` if `module`'s `capability` has been revoked in `profile`.
    pub fn is_revoked(&self, module: &str, capability: &str, profile: &str) -> bool {
        self.records
            .read()
            .expect("revocation registry lock must not be poisoned")
            .iter()
            .any(|r| r.module == module && r.capability == capability && r.profile == profile)
    }

    /// Borrowed read-only view of all recorded revocations.
    ///
    /// This returns a guard wrapper instead of a bare `&[RevocationRecord]`
    /// because cloned registries share runtime-mutable state. Returning a bare
    /// slice would let callers outlive the lock that protects the shared data.
    pub fn records(&self) -> RevocationRecords<'_> {
        RevocationRecords(
            self.records
                .read()
                .expect("revocation registry lock must not be poisoned"),
        )
    }

    /// Owned snapshot of all recorded revocations.
    pub fn records_snapshot(&self) -> Vec<RevocationRecord> {
        self.records().to_vec()
    }
}

// ── ProfilePolicy ────────────────────────────────────────────────────────

/// A named policy applied within a runtime profile.
///
/// Policies govern behaviour such as: payload redaction, unsafe surface
/// approval, capability rate limits, and audit verbosity.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ProfilePolicy {
    /// Machine-readable policy identifier (e.g., `"redact_payloads"`).
    pub name: String,
    /// Optional free-text configuration for this policy.
    pub config: Option<String>,
}

// ── SecretEntry ──────────────────────────────────────────────────────────

/// A mapping from a logical secret identifier to a vault location.
///
/// Secret access is capability-controlled; the host resolves the vault
/// path and injects the secret value without exposing it to WASM.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SecretEntry {
    /// Logical identifier used in the module (e.g., `"StripeApiKey"`).
    pub secret_id: String,
    /// Vault path or reference where the secret is stored.
    pub vault_path: String,
}

// ── AuditConfig ──────────────────────────────────────────────────────────

/// Configuration for audit logging within a runtime profile.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct AuditConfig {
    /// If `true`, payload bytes are redacted (only hashes are logged).
    pub redact_payloads: bool,
    /// Verbosity label (e.g., `"full"`, `"redacted"`, `"minimal"`).
    pub log_level: String,
}

// ── ReplayConfig ─────────────────────────────────────────────────────────

/// Configuration for deterministic replay mode.
///
/// When replay is active, the runtime uses recorded capability responses
/// instead of dispatching to live handlers, and optionally verifies that
/// the output hashes match the recorded ones.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ReplayConfig {
    /// Trace ID of the recorded session to replay.
    pub trace_id: Option<String>,
    /// If `true`, use recorded capability responses instead of live dispatch.
    pub use_recorded_responses: bool,
    /// If `true`, verify that replayed output hashes match recorded hashes.
    pub verify_output_hashes: bool,
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
/// - **Policies**: named policies governing runtime behaviour.
/// - **Secrets mapping**: logical secret IDs → vault paths.
/// - **Audit config**: payload redaction and log verbosity.
/// - **Replay config**: deterministic replay mode settings.
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

    /// Named policies applied in this profile.
    ///
    /// Policies govern payload redaction, unsafe-surface approval,
    /// rate limits, and other runtime-enforced behaviours.
    policies: Vec<ProfilePolicy>,

    /// Logical secret ID → vault path mappings for this profile.
    ///
    /// The host resolves vault paths when a `secret.read` capability is
    /// granted; the WASM module never receives the raw secret value.
    secrets_mapping: Vec<SecretEntry>,

    /// Audit logging configuration for this profile.
    ///
    /// Controls payload redaction and log verbosity for all capability calls
    /// executed under this profile.
    audit_config: Option<AuditConfig>,

    /// Deterministic replay configuration for this profile.
    ///
    /// When set, the runtime uses recorded responses instead of live dispatch
    /// and can verify output hashes against the recording.
    replay_config: Option<ReplayConfig>,
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
            policies: Vec::new(),
            secrets_mapping: Vec::new(),
            audit_config: None,
            replay_config: None,
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

    /// Attach named policies to this profile (builder pattern).
    pub fn with_policies(mut self, policies: Vec<ProfilePolicy>) -> Self {
        self.policies = policies;
        self
    }

    /// Attach a secrets mapping to this profile (builder pattern).
    pub fn with_secrets_mapping(mut self, secrets: Vec<SecretEntry>) -> Self {
        self.secrets_mapping = secrets;
        self
    }

    /// Attach audit configuration to this profile (builder pattern).
    pub fn with_audit_config(mut self, config: AuditConfig) -> Self {
        self.audit_config = Some(config);
        self
    }

    /// Attach replay configuration to this profile (builder pattern).
    pub fn with_replay_config(mut self, config: ReplayConfig) -> Self {
        self.replay_config = Some(config);
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

    /// Named policies applied in this profile.
    pub fn policies(&self) -> &[ProfilePolicy] {
        &self.policies
    }

    /// Secrets mapping for this profile.
    pub fn secrets_mapping(&self) -> &[SecretEntry] {
        &self.secrets_mapping
    }

    /// Audit configuration, if set.
    pub fn audit_config(&self) -> Option<&AuditConfig> {
        self.audit_config.as_ref()
    }

    /// Replay configuration, if set.
    pub fn replay_config(&self) -> Option<&ReplayConfig> {
        self.replay_config.as_ref()
    }

    /// Return `true` if `module` is granted `capability`.
    ///
    /// Both the module name and capability ID must match — a grant for
    /// `module.checkout` does NOT apply to `module.admin`, even for the
    /// same capability.  This enforces the per-profile, per-module scope
    /// described in `docs/runtime.md §Grants per profile`.
    ///
    /// Performs a linear scan; the grants list is expected to be small
    /// (single-digit items in practice).
    pub fn grants_capability(&self, module: &str, capability: &CapabilityId) -> bool {
        self.grants
            .iter()
            .any(|g| g.module.as_str() == module && &g.capability == capability)
    }

    /// `true` if preflight must verify handler binding for all grants.
    pub fn require_handler_binding(&self) -> bool {
        self.require_handler_binding
    }
}
