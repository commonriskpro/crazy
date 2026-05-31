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

use crate::manifest::CapabilityManifest;

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

// ── Runtime capability diagnostics ──────────────────────────────────────

/// Deterministic key for manifest capabilities missing a profile grant.
pub const RUNTIME_CAPABILITY_DIAGNOSTIC_KEY_MISSING_GRANT: &str =
    "runtime.capability.missing_grant";
/// Deterministic key for direct capability calls denied by the active profile.
pub const RUNTIME_CAPABILITY_DIAGNOSTIC_KEY_DENIED_CAPABILITY: &str = "runtime.capability.denied";
/// Deterministic key for capability calls attempted without an active profile.
pub const RUNTIME_CAPABILITY_DIAGNOSTIC_KEY_AMBIENT_ACCESS: &str =
    "runtime.capability.ambient_access";
/// Deterministic key for capability calls granted to another module/profile scope.
pub const RUNTIME_CAPABILITY_DIAGNOSTIC_KEY_PROFILE_MISMATCH: &str =
    "runtime.capability.profile_mismatch";

/// Production-safe capability enforcement diagnostic kind.
///
/// Declaration order is the canonical batch ordering: ambient access attempts
/// first, then profile mismatches, missing startup grants, and finally direct
/// denied capability calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RuntimeCapabilityDiagnosticKind {
    /// Runtime access was attempted without an active profile/module binding.
    AmbientAccessAttempt,
    /// The active profile grants the capability, but not to the calling module.
    ProfileMismatch,
    /// A manifest-required capability is absent from the profile grants.
    MissingGrant,
    /// A direct capability call was denied by the active profile.
    DeniedCapability,
}

impl RuntimeCapabilityDiagnosticKind {
    /// Stable machine-readable key for this diagnostic kind.
    pub const fn diagnostic_key(self) -> &'static str {
        match self {
            RuntimeCapabilityDiagnosticKind::AmbientAccessAttempt => {
                RUNTIME_CAPABILITY_DIAGNOSTIC_KEY_AMBIENT_ACCESS
            }
            RuntimeCapabilityDiagnosticKind::ProfileMismatch => {
                RUNTIME_CAPABILITY_DIAGNOSTIC_KEY_PROFILE_MISMATCH
            }
            RuntimeCapabilityDiagnosticKind::MissingGrant => {
                RUNTIME_CAPABILITY_DIAGNOSTIC_KEY_MISSING_GRANT
            }
            RuntimeCapabilityDiagnosticKind::DeniedCapability => {
                RUNTIME_CAPABILITY_DIAGNOSTIC_KEY_DENIED_CAPABILITY
            }
        }
    }
}

/// Stable redacted descriptor for one runtime capability enforcement issue.
///
/// The descriptor never includes raw profile names, module names, operations,
/// payload bytes, or capability targets such as secret IDs. Capability families
/// with safe lowercase namespace shapes are retained; unsafe names collapse to
/// a fixed opaque descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCapabilityDiagnostic {
    /// Canonical diagnostic kind.
    pub kind: RuntimeCapabilityDiagnosticKind,
    /// Stable machine-readable key for dashboards/support triage.
    pub diagnostic_key: &'static str,
    /// Redacted capability shape, e.g. `secret.read:<redacted>`.
    pub capability: String,
    /// Redacted profile presence only, never the profile name.
    pub profile: &'static str,
    /// Redacted module presence only, never the module name.
    pub module: &'static str,
}

impl RuntimeCapabilityDiagnostic {
    fn new(
        kind: RuntimeCapabilityDiagnosticKind,
        capability: &CapabilityId,
        profile: Option<&RuntimeProfile>,
        module: Option<&str>,
    ) -> Self {
        RuntimeCapabilityDiagnostic {
            kind,
            diagnostic_key: kind.diagnostic_key(),
            capability: redacted_capability_descriptor(capability),
            profile: if profile.is_some() {
                "profile:<active>"
            } else {
                "profile:<ambient>"
            },
            module: if module.is_some_and(|m| !m.is_empty()) {
                "module:<bound>"
            } else {
                "module:<ambient>"
            },
        }
    }

    fn sort_key(&self) -> (RuntimeCapabilityDiagnosticKind, &str, &str, &str) {
        (
            self.kind,
            self.capability.as_str(),
            self.profile,
            self.module,
        )
    }
}

/// Return a stable redacted capability descriptor for diagnostics.
///
/// Capability targets after `:` are always redacted. Families are retained only
/// when they are low-cardinality lowercase namespace labels such as
/// `secret.read` or `network.egress`; otherwise the descriptor is opaque.
pub fn redacted_capability_descriptor(capability: &CapabilityId) -> String {
    let raw = capability.as_str();
    if raw.is_empty() {
        return "capability:<empty>".to_string();
    }

    let (family, has_target) = match raw.split_once(':') {
        Some((family, _)) => (family, true),
        None => (raw, false),
    };

    if !is_safe_capability_family(family) {
        return "capability:<opaque>".to_string();
    }

    if has_target {
        format!("{family}:<redacted>")
    } else {
        format!("{family}:<none>")
    }
}

fn is_safe_capability_family(family: &str) -> bool {
    !family.is_empty()
        && family.split('.').all(|segment| !segment.is_empty())
        && family.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn sort_and_dedup_capability_diagnostics(
    mut diagnostics: Vec<RuntimeCapabilityDiagnostic>,
) -> Vec<RuntimeCapabilityDiagnostic> {
    diagnostics.sort_by(|left, right| left.sort_key().cmp(&right.sort_key()));
    diagnostics.dedup_by(|left, right| left.sort_key() == right.sort_key());
    diagnostics
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

// ── AssumptionStatus ─────────────────────────────────────────────────────

/// Lifecycle status of a profile-level assumption.
///
/// Used by preflight stage 7 to enforce that all assumptions declared by a
/// [`RuntimeProfile`] are active and not expired before allowing startup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssumptionStatus {
    /// The assumption is currently active and valid.
    Active,
    /// The assumption exists but is not currently active.
    ///
    /// Preflight stage 7 rejects profiles that depend on an inactive assumption.
    Inactive,
    /// The assumption has been explicitly marked as expired.
    ///
    /// Preflight stage 7 rejects profiles that depend on an expired assumption.
    Expired,
}

// ── ProfileAssumption ─────────────────────────────────────────────────────

/// A named assumption that must be active/not-expired for a profile to start.
///
/// Profiles that declare assumptions have them checked during preflight
/// stage 7.  If any assumption is [`AssumptionStatus::Expired`],
/// [`AssumptionStatus::Inactive`], or has an `expires_at` timestamp in the
/// past, preflight fails with
/// [`PreflightFailure::AssumptionExpired`](crate::error::PreflightFailure::AssumptionExpired).
///
/// Profiles with an empty assumption list (the default) skip stage 7 entirely,
/// preserving backward compatibility.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileAssumption {
    /// Machine-readable identifier for this assumption (e.g. `"payment-api-v2"`).
    pub id: String,
    /// Current lifecycle status.
    pub status: AssumptionStatus,
    /// Optional wall-clock expiry time.
    ///
    /// If `Some(t)` and `t < SystemTime::now()`, the assumption is treated as
    /// expired regardless of `status`.
    pub expires_at: Option<std::time::SystemTime>,
}

impl ProfileAssumption {
    /// Create an active assumption with no expiry deadline.
    pub fn active(id: impl Into<String>) -> Self {
        ProfileAssumption {
            id: id.into(),
            status: AssumptionStatus::Active,
            expires_at: None,
        }
    }

    /// Create an assumption that expires at the given wall-clock time.
    ///
    /// The status is set to [`AssumptionStatus::Active`]; if `expires_at` is
    /// in the past, preflight stage 7 will still reject the profile.
    pub fn active_until(id: impl Into<String>, expires_at: std::time::SystemTime) -> Self {
        ProfileAssumption {
            id: id.into(),
            status: AssumptionStatus::Active,
            expires_at: Some(expires_at),
        }
    }

    /// Create an already-expired assumption.
    pub fn expired(id: impl Into<String>) -> Self {
        ProfileAssumption {
            id: id.into(),
            status: AssumptionStatus::Expired,
            expires_at: None,
        }
    }

    /// Create an inactive assumption.
    pub fn inactive(id: impl Into<String>) -> Self {
        ProfileAssumption {
            id: id.into(),
            status: AssumptionStatus::Inactive,
            expires_at: None,
        }
    }
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

    /// Optional minimum trust tier for bound handlers.
    ///
    /// `None` disables the handler trust gate entirely (preserves existing
    /// behaviour for callers that don't opt in to handler trust checking).
    /// `Some(level)` causes preflight to reject any handler that serves a
    /// granted capability but whose `trust_level()` does not satisfy `level`.
    ///
    /// Corresponds to `docs/runtime.md §Handler execution model` rule:
    /// "unverified handler blocked in prod/critical unless policy exception".
    min_handler_trust: Option<TrustLevel>,

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

    /// Assumptions that must be active/not-expired for this profile to start.
    ///
    /// Checked during preflight stage 7.  An empty list (the default) disables
    /// the stage entirely so existing profiles are unaffected.
    assumptions: Vec<ProfileAssumption>,
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
            min_handler_trust: None,
            policies: Vec::new(),
            secrets_mapping: Vec::new(),
            audit_config: None,
            replay_config: None,
            assumptions: Vec::new(),
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

    /// Set the minimum handler trust tier for this profile.
    ///
    /// Consumes `self` and returns a new `RuntimeProfile` with
    /// `min_handler_trust` set to `Some(level)`.  Use this builder method
    /// to opt in to handler trust gating without changing the `new`
    /// constructor signature.
    ///
    /// When set, preflight will fail with
    /// [`PreflightFailure::HandlerTrustViolation`](crate::error::PreflightFailure::HandlerTrustViolation)
    /// for any bound handler whose `trust_level()` does not satisfy `level`.
    pub fn with_min_handler_trust(mut self, level: TrustLevel) -> Self {
        self.min_handler_trust = Some(level);
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

    /// Declare assumptions that preflight stage 7 must verify are active.
    ///
    /// Replaces any previously set assumptions list.  Profiles with an empty
    /// list (the default) skip stage 7 entirely — backward compatible.
    pub fn with_assumptions(mut self, assumptions: Vec<ProfileAssumption>) -> Self {
        self.assumptions = assumptions;
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

    /// Assumptions declared for this profile.
    ///
    /// An empty slice means stage 7 preflight is disabled (default).
    pub fn assumptions(&self) -> &[ProfileAssumption] {
        &self.assumptions
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

    /// Return `true` if any module in this profile is granted `capability`.
    ///
    /// This is used only for diagnostics so the runtime can distinguish a
    /// capability that is unknown to the profile from one granted to a
    /// different module scope.
    pub fn grants_capability_to_any_module(&self, capability: &CapabilityId) -> bool {
        self.grants.iter().any(|g| &g.capability == capability)
    }

    /// Return stable redacted diagnostics for manifest requirements missing grants.
    ///
    /// Output is canonical and de-duplicated by diagnostic kind and redacted
    /// descriptor so batches are stable even when manifests repeat capability
    /// requirements in different declaration orders.
    pub fn capability_diagnostics_for_manifest(
        &self,
        manifest: &CapabilityManifest,
    ) -> Vec<RuntimeCapabilityDiagnostic> {
        sort_and_dedup_capability_diagnostics(
            manifest
                .requires
                .iter()
                .filter(|capability| !self.grants_capability(&manifest.module, capability))
                .map(|capability| {
                    RuntimeCapabilityDiagnostic::new(
                        RuntimeCapabilityDiagnosticKind::MissingGrant,
                        capability,
                        Some(self),
                        Some(&manifest.module),
                    )
                })
                .collect(),
        )
    }

    /// Return a stable redacted diagnostic for a runtime capability access.
    ///
    /// `None` means the access is granted for the provided module. Denials are
    /// classified without leaking raw profile/module/capability target names.
    pub fn capability_diagnostic_for_access(
        &self,
        module: Option<&str>,
        capability: &CapabilityId,
    ) -> Option<RuntimeCapabilityDiagnostic> {
        match module {
            Some(module) if self.grants_capability(module, capability) => None,
            Some(module) if self.grants_capability_to_any_module(capability) => {
                Some(RuntimeCapabilityDiagnostic::new(
                    RuntimeCapabilityDiagnosticKind::ProfileMismatch,
                    capability,
                    Some(self),
                    Some(module),
                ))
            }
            Some(module) => Some(RuntimeCapabilityDiagnostic::new(
                RuntimeCapabilityDiagnosticKind::DeniedCapability,
                capability,
                Some(self),
                Some(module),
            )),
            None => Some(RuntimeCapabilityDiagnostic::new(
                RuntimeCapabilityDiagnosticKind::AmbientAccessAttempt,
                capability,
                Some(self),
                None,
            )),
        }
    }

    /// Batch form of [`RuntimeProfile::capability_diagnostic_for_access`].
    ///
    /// Returned diagnostics are canonical and de-duplicated by redacted
    /// descriptor; raw operation names or payloads are intentionally not part
    /// of this API.
    pub fn capability_diagnostics_for_accesses<'a>(
        &self,
        accesses: impl IntoIterator<Item = (Option<&'a str>, &'a CapabilityId)>,
    ) -> Vec<RuntimeCapabilityDiagnostic> {
        sort_and_dedup_capability_diagnostics(
            accesses
                .into_iter()
                .filter_map(|(module, capability)| {
                    self.capability_diagnostic_for_access(module, capability)
                })
                .collect(),
        )
    }

    /// `true` if preflight must verify handler binding for all grants.
    pub fn require_handler_binding(&self) -> bool {
        self.require_handler_binding
    }

    /// Minimum handler trust tier required by this profile.
    ///
    /// `None` means the handler trust gate is disabled.
    pub fn min_handler_trust(&self) -> Option<TrustLevel> {
        self.min_handler_trust
    }
}
