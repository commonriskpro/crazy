// ── ail-runtime::audit ───────────────────────────────────────────────────
//
// Audit event types and in-memory ordered log.
//
// Spec invariants:
//   - Exactly one event is appended per preflight call.
//   - Events are ordered by insertion (Vec<AuditEvent>).
//   - Payloads MUST NOT contain raw WASM bytes, user data, or secrets.
//   - Events MAY include hash digests and denied capability names.
//
// AuditEvent::CapabilityCallExecuted carries the 13-field set described in
// docs/runtime.md §Audit log:
//   timestamp, profile, module, function, capability, operation, handler,
//   input_hash, output_hash, result_state, duration, trace_id,
//   verification_report_hash

use crate::error::PreflightFailure;
use crate::host::TraceContext;
use crate::profile::CapabilityId;

// ── Stable denial categories ─────────────────────────────────────────────

/// Runtime denial category for capability calls denied because the active
/// profile does not grant the requested capability.
pub const DENIAL_CATEGORY_CAPABILITY_NOT_GRANTED: &str = "capability.not_granted";
/// Runtime denial category for capability calls blocked by a revocation record.
pub const DENIAL_CATEGORY_CAPABILITY_REVOKED: &str = "capability.revoked";
/// Runtime denial category for granted capabilities with no bound handler.
pub const DENIAL_CATEGORY_HANDLER_NOT_BOUND: &str = "handler.not_bound";
/// Runtime denial category for input payload size limit failures.
pub const DENIAL_CATEGORY_LIMIT_PAYLOAD_SIZE: &str = "limit.payload_size";
/// Runtime denial category for Wasmtime memory resource limit failures.
pub const DENIAL_CATEGORY_LIMIT_MEMORY: &str = "limit.memory";
/// Runtime denial category for Wasmtime fuel resource limit failures.
pub const DENIAL_CATEGORY_LIMIT_FUEL: &str = "limit.fuel";
/// Runtime denial category for wall-clock timeout limit failures.
pub const DENIAL_CATEGORY_LIMIT_TIMEOUT: &str = "limit.timeout";
/// Runtime denial category for max capability call count failures.
pub const DENIAL_CATEGORY_LIMIT_MAX_CAPABILITY_CALLS: &str = "limit.max_capability_calls";
/// Runtime denial category for rate-limit failures.
pub const DENIAL_CATEGORY_LIMIT_RATE: &str = "limit.rate";
/// Runtime denial category for concurrent call limit failures.
pub const DENIAL_CATEGORY_LIMIT_CONCURRENCY: &str = "limit.concurrency";
/// Runtime denial category for call-depth/recursion limit failures.
pub const DENIAL_CATEGORY_LIMIT_RECURSION_DEPTH: &str = "limit.recursion_depth";
/// Runtime denial category for output payload size limit failures.
pub const DENIAL_CATEGORY_LIMIT_OUTPUT_SIZE: &str = "limit.output_size";
/// Runtime denial category for runtime input schema validation failures.
pub const DENIAL_CATEGORY_SCHEMA_INPUT: &str = "schema.input";
/// Runtime denial category for runtime output schema validation failures.
pub const DENIAL_CATEGORY_SCHEMA_OUTPUT: &str = "schema.output";
/// Runtime denial category for host/WASM payload boundary decode failures.
pub const DENIAL_CATEGORY_PAYLOAD_DECODE: &str = "payload.decode";
/// Runtime denial category for secret access when the mapped secret cannot be found.
pub const SECRET_AUDIT_CATEGORY_NOT_FOUND: &str = "secret.not_found";
/// Runtime denial category for secret access when the provider is unavailable.
pub const SECRET_AUDIT_CATEGORY_PROVIDER_UNAVAILABLE: &str = "secret.provider_unavailable";
/// Runtime denial category for secret access using an unsupported operation.
pub const SECRET_AUDIT_CATEGORY_UNSUPPORTED_OPERATION: &str = "secret.unsupported_operation";
/// Runtime denial category for malformed secret access capability identifiers.
pub const SECRET_AUDIT_CATEGORY_MALFORMED_CAPABILITY: &str = "secret.malformed_capability";

// ── Stable secret access shape descriptors ──────────────────────────────

/// Redacted shape descriptor for well-formed `secret.read:<id>` accesses.
pub const SECRET_ACCESS_SHAPE_READ: &str = "secret.read:<redacted>";
/// Redacted shape descriptor for malformed empty `secret.read:` accesses.
pub const SECRET_ACCESS_SHAPE_MALFORMED: &str = "secret.read:<malformed>";

// ── Stable profile policy denial shape descriptors ─────────────────────

/// Redacted shape descriptor for profile capability grants denied by default.
pub const PROFILE_POLICY_DENIAL_SHAPE_CAPABILITY_NOT_GRANTED: &str =
    "profile.policy:<capability_not_granted>";
/// Redacted shape descriptor for profile capability grants denied by revocation.
pub const PROFILE_POLICY_DENIAL_SHAPE_CAPABILITY_REVOKED: &str =
    "profile.policy:<capability_revoked>";

/// Deterministic diagnostic key for profile capability grants denied by default.
pub const PROFILE_POLICY_DENIAL_DIAGNOSTIC_KEY_CAPABILITY_NOT_GRANTED: &str =
    "profile.policy.capability_not_granted";
/// Deterministic diagnostic key for profile capability grants denied by revocation.
pub const PROFILE_POLICY_DENIAL_DIAGNOSTIC_KEY_CAPABILITY_REVOKED: &str =
    "profile.policy.capability_revoked";

// ── Stable limit denial shape descriptors ───────────────────────────────

/// Redacted shape descriptor for memory-limit denials.
pub const LIMIT_DENIAL_SHAPE_MEMORY: &str = "runtime.limit:<memory>";
/// Redacted shape descriptor for fuel-limit denials.
pub const LIMIT_DENIAL_SHAPE_FUEL: &str = "runtime.limit:<fuel>";
/// Redacted shape descriptor for wall-clock timeout denials.
pub const LIMIT_DENIAL_SHAPE_TIME: &str = "runtime.limit:<time>";
/// Redacted shape descriptor for max capability call denials.
pub const LIMIT_DENIAL_SHAPE_MAX_CAPABILITY_CALLS: &str = "dispatch.limit:<max_capability_calls>";
/// Redacted shape descriptor for fixed-window rate limit denials.
pub const LIMIT_DENIAL_SHAPE_RATE: &str = "dispatch.limit:<rate>";
/// Redacted shape descriptor for concurrent in-flight call denials.
pub const LIMIT_DENIAL_SHAPE_CONCURRENCY: &str = "dispatch.limit:<concurrency>";
/// Redacted shape descriptor for call-depth/recursion denials.
pub const LIMIT_DENIAL_SHAPE_CALL_DEPTH: &str = "dispatch.limit:<call_depth>";
/// Redacted shape descriptor for input payload size denials.
pub const LIMIT_DENIAL_SHAPE_PAYLOAD_SIZE: &str = "dispatch.limit:<payload_size>";
/// Redacted shape descriptor for output payload size denials.
pub const LIMIT_DENIAL_SHAPE_OUTPUT_SIZE: &str = "dispatch.limit:<output_size>";

/// Deterministic diagnostic key for memory-limit denials.
pub const LIMIT_DENIAL_DIAGNOSTIC_KEY_MEMORY: &str = "runtime.limit.memory";
/// Deterministic diagnostic key for fuel-limit denials.
pub const LIMIT_DENIAL_DIAGNOSTIC_KEY_FUEL: &str = "runtime.limit.fuel";
/// Deterministic diagnostic key for wall-clock timeout denials.
pub const LIMIT_DENIAL_DIAGNOSTIC_KEY_TIME: &str = "runtime.limit.time";
/// Deterministic diagnostic key for max capability call denials.
pub const LIMIT_DENIAL_DIAGNOSTIC_KEY_MAX_CAPABILITY_CALLS: &str =
    "dispatch.limit.max_capability_calls";
/// Deterministic diagnostic key for fixed-window rate limit denials.
pub const LIMIT_DENIAL_DIAGNOSTIC_KEY_RATE: &str = "dispatch.limit.rate";
/// Deterministic diagnostic key for concurrent in-flight call denials.
pub const LIMIT_DENIAL_DIAGNOSTIC_KEY_CONCURRENCY: &str = "dispatch.limit.concurrency";
/// Deterministic diagnostic key for call-depth/recursion denials.
pub const LIMIT_DENIAL_DIAGNOSTIC_KEY_CALL_DEPTH: &str = "dispatch.limit.call_depth";
/// Deterministic diagnostic key for input payload size denials.
pub const LIMIT_DENIAL_DIAGNOSTIC_KEY_PAYLOAD_SIZE: &str = "dispatch.limit.payload_size";
/// Deterministic diagnostic key for output payload size denials.
pub const LIMIT_DENIAL_DIAGNOSTIC_KEY_OUTPUT_SIZE: &str = "dispatch.limit.output_size";

pub(crate) fn denial_category(category: &'static str) -> Option<String> {
    Some(category.to_string())
}

// ── Stable replay mismatch categories ────────────────────────────────────

/// Replay/audit category for calls that have no recorded capability response.
pub const REPLAY_MISMATCH_MISSING_RECORDING: &str = "replay.missing_recording";
/// Replay/audit category for recorded responses whose output hash no longer matches.
pub const REPLAY_MISMATCH_HASH_MISMATCH: &str = "replay.hash_mismatch";
/// Deterministic diagnostic key for missing replay recordings.
pub const REPLAY_MISMATCH_DIAGNOSTIC_KEY_MISSING_RECORDING: &str =
    "replay.mismatch.missing_recording";
/// Deterministic diagnostic key for replay output-hash mismatches.
pub const REPLAY_MISMATCH_DIAGNOSTIC_KEY_HASH_MISMATCH: &str = "replay.mismatch.hash_mismatch";

// ── Stable runtime issue descriptors ────────────────────────────────────

/// Coarse redacted axis for production runtime issue descriptors.
///
/// The declaration order is the canonical batch ordering used by
/// [`runtime_issue_descriptors_for_events`]: timeout, step, memory,
/// capability, then broader resource-policy issues.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RuntimeIssueAxis {
    /// Wall-clock timeout/deadline limit.
    Timeout,
    /// Execution step/fuel budget limit.
    Step,
    /// Linear memory growth/size limit.
    Memory,
    /// Capability grant/revocation policy denial.
    Capability,
    /// Other runtime resource-policy denial such as rate, payload, output,
    /// concurrency, handler binding/trust, package trust, or assumptions.
    ResourcePolicy,
}

/// Stable redacted descriptor for one class of runtime safety issue.
///
/// Descriptors intentionally carry only stable grouping metadata. They never
/// include profile names, module names, capability IDs, operation names,
/// payload bytes, configured thresholds, current usage, or raw trap text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RuntimeIssueDescriptor {
    /// Canonical coarse axis for sorting/grouping issue descriptors.
    pub axis: RuntimeIssueAxis,
    /// Stable machine-readable key for dashboards and support triage.
    pub diagnostic_key: &'static str,
    /// Redacted shape descriptor for UI grouping.
    pub shape: &'static str,
}

impl RuntimeIssueDescriptor {
    const fn new(
        axis: RuntimeIssueAxis,
        diagnostic_key: &'static str,
        shape: &'static str,
    ) -> Self {
        RuntimeIssueDescriptor {
            axis,
            diagnostic_key,
            shape,
        }
    }
}

/// Redacted descriptor shape for generic resource-policy issues.
pub const RUNTIME_ISSUE_SHAPE_RESOURCE_POLICY: &str = "runtime.policy:<resource>";
/// Deterministic diagnostic key for generic resource-policy issues.
pub const RUNTIME_ISSUE_DIAGNOSTIC_KEY_RESOURCE_POLICY: &str = "runtime.policy.resource";

// ── Stable transaction lifecycle categories ──────────────────────────────

/// Transaction/audit category for a pending transaction committed successfully.
pub const TRANSACTION_CATEGORY_COMMITTED: &str = "transaction.committed";
/// Transaction/audit category for a pending transaction rolled back successfully.
pub const TRANSACTION_CATEGORY_ROLLED_BACK: &str = "transaction.rolled_back";
/// Transaction/audit category for a commit request repeated after commit.
pub const TRANSACTION_CATEGORY_COMMIT_ALREADY_COMMITTED: &str =
    "transaction.commit_already_committed";
/// Transaction/audit category for a commit request made after rollback.
pub const TRANSACTION_CATEGORY_COMMIT_AFTER_ROLLBACK: &str = "transaction.commit_after_rollback";
/// Transaction/audit category for a rollback request made after commit.
pub const TRANSACTION_CATEGORY_ROLLBACK_AFTER_COMMIT: &str = "transaction.rollback_after_commit";
/// Transaction/audit category for a rollback request repeated after rollback.
pub const TRANSACTION_CATEGORY_ROLLBACK_ALREADY_ROLLED_BACK: &str =
    "transaction.rollback_already_rolled_back";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProfilePolicyDenialKind {
    CapabilityNotGranted,
    CapabilityRevoked,
}

impl ProfilePolicyDenialKind {
    fn shape(self) -> &'static str {
        match self {
            ProfilePolicyDenialKind::CapabilityNotGranted => {
                PROFILE_POLICY_DENIAL_SHAPE_CAPABILITY_NOT_GRANTED
            }
            ProfilePolicyDenialKind::CapabilityRevoked => {
                PROFILE_POLICY_DENIAL_SHAPE_CAPABILITY_REVOKED
            }
        }
    }

    fn diagnostic_key(self) -> &'static str {
        match self {
            ProfilePolicyDenialKind::CapabilityNotGranted => {
                PROFILE_POLICY_DENIAL_DIAGNOSTIC_KEY_CAPABILITY_NOT_GRANTED
            }
            ProfilePolicyDenialKind::CapabilityRevoked => {
                PROFILE_POLICY_DENIAL_DIAGNOSTIC_KEY_CAPABILITY_REVOKED
            }
        }
    }
}

fn profile_policy_denial_kind_from_category(category: &str) -> Option<ProfilePolicyDenialKind> {
    match category {
        DENIAL_CATEGORY_CAPABILITY_NOT_GRANTED => {
            Some(ProfilePolicyDenialKind::CapabilityNotGranted)
        }
        DENIAL_CATEGORY_CAPABILITY_REVOKED => Some(ProfilePolicyDenialKind::CapabilityRevoked),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LimitDenialKind {
    Memory,
    Fuel,
    Time,
    MaxCapabilityCalls,
    Rate,
    Concurrency,
    CallDepth,
    PayloadSize,
    OutputSize,
}

impl LimitDenialKind {
    fn shape(self) -> &'static str {
        match self {
            LimitDenialKind::Memory => LIMIT_DENIAL_SHAPE_MEMORY,
            LimitDenialKind::Fuel => LIMIT_DENIAL_SHAPE_FUEL,
            LimitDenialKind::Time => LIMIT_DENIAL_SHAPE_TIME,
            LimitDenialKind::MaxCapabilityCalls => LIMIT_DENIAL_SHAPE_MAX_CAPABILITY_CALLS,
            LimitDenialKind::Rate => LIMIT_DENIAL_SHAPE_RATE,
            LimitDenialKind::Concurrency => LIMIT_DENIAL_SHAPE_CONCURRENCY,
            LimitDenialKind::CallDepth => LIMIT_DENIAL_SHAPE_CALL_DEPTH,
            LimitDenialKind::PayloadSize => LIMIT_DENIAL_SHAPE_PAYLOAD_SIZE,
            LimitDenialKind::OutputSize => LIMIT_DENIAL_SHAPE_OUTPUT_SIZE,
        }
    }

    fn diagnostic_key(self) -> &'static str {
        match self {
            LimitDenialKind::Memory => LIMIT_DENIAL_DIAGNOSTIC_KEY_MEMORY,
            LimitDenialKind::Fuel => LIMIT_DENIAL_DIAGNOSTIC_KEY_FUEL,
            LimitDenialKind::Time => LIMIT_DENIAL_DIAGNOSTIC_KEY_TIME,
            LimitDenialKind::MaxCapabilityCalls => LIMIT_DENIAL_DIAGNOSTIC_KEY_MAX_CAPABILITY_CALLS,
            LimitDenialKind::Rate => LIMIT_DENIAL_DIAGNOSTIC_KEY_RATE,
            LimitDenialKind::Concurrency => LIMIT_DENIAL_DIAGNOSTIC_KEY_CONCURRENCY,
            LimitDenialKind::CallDepth => LIMIT_DENIAL_DIAGNOSTIC_KEY_CALL_DEPTH,
            LimitDenialKind::PayloadSize => LIMIT_DENIAL_DIAGNOSTIC_KEY_PAYLOAD_SIZE,
            LimitDenialKind::OutputSize => LIMIT_DENIAL_DIAGNOSTIC_KEY_OUTPUT_SIZE,
        }
    }
}

fn limit_denial_kind_from_category(category: &str) -> Option<LimitDenialKind> {
    match category {
        DENIAL_CATEGORY_LIMIT_MEMORY => Some(LimitDenialKind::Memory),
        DENIAL_CATEGORY_LIMIT_FUEL => Some(LimitDenialKind::Fuel),
        DENIAL_CATEGORY_LIMIT_TIMEOUT => Some(LimitDenialKind::Time),
        DENIAL_CATEGORY_LIMIT_MAX_CAPABILITY_CALLS => Some(LimitDenialKind::MaxCapabilityCalls),
        DENIAL_CATEGORY_LIMIT_RATE => Some(LimitDenialKind::Rate),
        DENIAL_CATEGORY_LIMIT_CONCURRENCY => Some(LimitDenialKind::Concurrency),
        DENIAL_CATEGORY_LIMIT_RECURSION_DEPTH => Some(LimitDenialKind::CallDepth),
        DENIAL_CATEGORY_LIMIT_PAYLOAD_SIZE => Some(LimitDenialKind::PayloadSize),
        DENIAL_CATEGORY_LIMIT_OUTPUT_SIZE => Some(LimitDenialKind::OutputSize),
        _ => None,
    }
}

fn limit_denial_descriptor(kind: LimitDenialKind) -> RuntimeIssueDescriptor {
    let axis = match kind {
        LimitDenialKind::Time => RuntimeIssueAxis::Timeout,
        LimitDenialKind::Fuel => RuntimeIssueAxis::Step,
        LimitDenialKind::Memory => RuntimeIssueAxis::Memory,
        LimitDenialKind::MaxCapabilityCalls
        | LimitDenialKind::Rate
        | LimitDenialKind::Concurrency
        | LimitDenialKind::CallDepth
        | LimitDenialKind::PayloadSize
        | LimitDenialKind::OutputSize => RuntimeIssueAxis::ResourcePolicy,
    };
    RuntimeIssueDescriptor::new(axis, kind.diagnostic_key(), kind.shape())
}

fn profile_policy_denial_descriptor(kind: ProfilePolicyDenialKind) -> RuntimeIssueDescriptor {
    RuntimeIssueDescriptor::new(
        RuntimeIssueAxis::Capability,
        kind.diagnostic_key(),
        kind.shape(),
    )
}

fn resource_policy_issue_descriptor() -> RuntimeIssueDescriptor {
    RuntimeIssueDescriptor::new(
        RuntimeIssueAxis::ResourcePolicy,
        RUNTIME_ISSUE_DIAGNOSTIC_KEY_RESOURCE_POLICY,
        RUNTIME_ISSUE_SHAPE_RESOURCE_POLICY,
    )
}

fn runtime_issue_descriptor_from_preflight_failure(
    reason: &PreflightFailure,
) -> Option<RuntimeIssueDescriptor> {
    match reason {
        PreflightFailure::CapabilityDenied { .. } => Some(profile_policy_denial_descriptor(
            ProfilePolicyDenialKind::CapabilityNotGranted,
        )),
        PreflightFailure::ResourceLimitExceeded { reason } => {
            limit_denial_kind_from_resource_reason(reason).map(limit_denial_descriptor)
        }
        PreflightFailure::PackageTrustViolation { .. }
        | PreflightFailure::UnsafePackageNotApproved { .. }
        | PreflightFailure::PackageVerificationEvidenceInvalid { .. }
        | PreflightFailure::HandlerNotBound { .. }
        | PreflightFailure::HandlerTrustViolation { .. }
        | PreflightFailure::AssumptionExpired { .. } => Some(resource_policy_issue_descriptor()),
        PreflightFailure::HashMismatch { .. } | PreflightFailure::WasmValidationError(_) => None,
    }
}

fn runtime_issue_descriptor_from_denial_category(category: &str) -> Option<RuntimeIssueDescriptor> {
    limit_denial_kind_from_category(category)
        .map(limit_denial_descriptor)
        .or_else(|| {
            profile_policy_denial_kind_from_category(category).map(profile_policy_denial_descriptor)
        })
        .or_else(|| match category {
            DENIAL_CATEGORY_HANDLER_NOT_BOUND => Some(resource_policy_issue_descriptor()),
            _ => None,
        })
}

fn limit_denial_kind_from_resource_reason(reason: &str) -> Option<LimitDenialKind> {
    let normalized = reason.to_ascii_lowercase();
    if normalized.contains("memory") {
        Some(LimitDenialKind::Memory)
    } else if normalized.contains("fuel") {
        Some(LimitDenialKind::Fuel)
    } else if normalized.contains("timeout")
        || normalized.contains("time limit")
        || normalized.contains("deadline")
    {
        Some(LimitDenialKind::Time)
    } else {
        None
    }
}

// ── AuditEvent ────────────────────────────────────────────────────────────

/// A single audit record.
///
/// Preflight events: one appended per `validate_and_instantiate` call.
/// Capability call events: one appended per `call_capability` call.
/// Payloads are redacted: no raw WASM bytes, no user data, no secrets.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuditEvent {
    /// All preflight checks passed; instantiation will proceed.
    PreflightPassed {
        /// Profile name that was checked.
        profile_name: String,
        /// BLAKE3 hex digest that was validated (not the bytes).
        module_hash: String,
    },

    /// At least one preflight check failed; instantiation was blocked.
    PreflightFailed {
        /// Profile name that was checked.
        profile_name: String,
        /// Capabilities that were denied (empty if failure was a hash mismatch).
        denied: Vec<CapabilityId>,
        /// Machine-readable failure reason (carries hashes/names, no raw bytes).
        reason: PreflightFailure,
    },

    /// A capability call was dispatched (or denied) via `call_capability`.
    ///
    /// Appended after every `call_capability` call regardless of outcome.
    /// Payload bytes are never included; only metadata is recorded.
    ///
    /// Field set matches `docs/runtime.md §Audit log` (13 fields):
    /// `timestamp`, `profile`, `module`, `function`, `capability`, `operation`,
    /// `handler`, `input_hash`, `output_hash`, `result_state`, `duration`,
    /// `trace_id`, `verification_report_hash`.
    CapabilityCallExecuted {
        // ── Identification ────────────────────────────────────────────────
        /// The capability that was requested.
        capability: CapabilityId,
        /// The specific operation within that capability.
        operation: String,
        /// Name of the handler that was dispatched to, or `"none"` if no
        /// handler was found or the call was denied before dispatch.
        handler_name: String,

        // ── Result ────────────────────────────────────────────────────────
        /// `true` if the handler returned `Ok(_)`, `false` otherwise.
        succeeded: bool,

        // ── Timing ────────────────────────────────────────────────────────
        /// Wall-clock duration of the dispatch in microseconds.
        duration_us: u64,
        /// Unix timestamp (microseconds since epoch) when the call started.
        timestamp: u64,

        // ── Context ───────────────────────────────────────────────────────
        /// Profile name under which this call was executed.
        profile: Option<String>,
        /// WASM module that requested the capability.
        module: Option<String>,
        /// Function within the module that initiated the call (if known).
        function: Option<String>,

        // ── Hashes (no raw payload data) ──────────────────────────────────
        /// BLAKE3 hex digest of the input payload bytes.
        input_hash: Option<String>,
        /// BLAKE3 hex digest of the response bytes (set only on success).
        output_hash: Option<String>,
        /// W3C-compatible trace ID for distributed trace correlation.
        trace_id: Option<String>,
        /// BLAKE3 hex digest of the verification report referenced by the
        /// active runtime profile.
        verification_report_hash: Option<String>,

        // ── Distributed trace ─────────────────────────────────────────────
        /// Distributed trace correlation for this capability call.
        ///
        /// `Some` when a [`TraceContext`] was active at call time; the context
        /// is a child span derived from the caller's span (same `trace_id`,
        /// new `span_id`, `parent_span_id` == caller's `span_id`).
        /// `None` when no trace context was set.
        trace_context: Option<TraceContext>,

        // ── Audit-only denial metadata (no secret data) ───────────────────
        /// Generic failure category set by the runtime or handler on denial.
        ///
        /// Runtime-controlled denials use stable machine-readable categories
        /// such as `"capability.not_granted"` or `"limit.payload_size"`.
        /// Handler-controlled denials may also provide opaque categories via
        /// [`HostError::CapabilityDeniedCategorized`](crate::abi::HostError::CapabilityDeniedCategorized),
        /// e.g. `"secret.not_found"`. Categories must not reveal secret IDs,
        /// vault paths, raw payloads, or other sensitive data.
        ///
        /// `None` on success or when a non-denial handler failure did not
        /// provide a category.
        denial_category: Option<String>,
    },

    /// A transaction group lifecycle transition was requested.
    ///
    /// The event shape is deterministic and redacted: it records the group
    /// name *shape* instead of the raw group name, counts of entries that need
    /// operational attention, stable statuses, and a stable category. It never
    /// includes user payloads, idempotency keys, refund capability names, or
    /// raw transaction labels.
    TransactionLifecycle {
        /// Redacted shape of the transaction group name, not the raw name.
        group_name_shape: String,
        /// Requested action, currently `"commit"` or `"rollback"`.
        action: String,
        /// Stable machine-readable transaction category.
        category: String,
        /// Status before the requested action.
        status_before: String,
        /// Status after the requested action.
        status_after: String,
        /// Number of capability entries tracked by this transaction.
        entry_count: usize,
        /// Number of entries marked non-rollbackable.
        non_rollbackable_count: usize,
        /// Number of non-rollbackable entries with explicit compensation.
        compensation_required_count: usize,
    },
}

impl AuditEvent {
    /// `true` if this event represents a successful preflight.
    pub fn is_passed(&self) -> bool {
        matches!(self, AuditEvent::PreflightPassed { .. })
    }

    /// `true` if this is a `CapabilityCallExecuted` event.
    pub fn is_capability_call(&self) -> bool {
        matches!(self, AuditEvent::CapabilityCallExecuted { .. })
    }

    /// Return a stable, redacted shape descriptor for profile-policy denials.
    ///
    /// The descriptor identifies only the policy denial class. It deliberately
    /// excludes profile names, module names, capability IDs, operation names,
    /// payloads, and handler details so security dashboards can group profile
    /// policy denials without leaking tenant-specific capability names.
    pub fn profile_policy_denial_shape(&self) -> Option<&'static str> {
        self.profile_policy_denial_kind()
            .map(ProfilePolicyDenialKind::shape)
    }

    /// Return a deterministic diagnostic key for profile-policy denials.
    pub fn profile_policy_denial_diagnostic_key(&self) -> Option<&'static str> {
        self.profile_policy_denial_kind()
            .map(ProfilePolicyDenialKind::diagnostic_key)
    }

    fn profile_policy_denial_kind(&self) -> Option<ProfilePolicyDenialKind> {
        match self {
            AuditEvent::PreflightFailed {
                reason: PreflightFailure::CapabilityDenied { .. },
                ..
            } => Some(ProfilePolicyDenialKind::CapabilityNotGranted),
            AuditEvent::CapabilityCallExecuted {
                denial_category: Some(category),
                ..
            } => profile_policy_denial_kind_from_category(category),
            _ => None,
        }
    }

    /// Return a stable, redacted shape descriptor for limit-denial events.
    ///
    /// The descriptor intentionally records only the limit axis (for example,
    /// memory, fuel, rate, or call depth), never configured thresholds, current
    /// usage, capability names, payloads, module names, or raw trap text.
    pub fn limit_denial_shape(&self) -> Option<&'static str> {
        self.limit_denial_kind().map(LimitDenialKind::shape)
    }

    /// Return a deterministic diagnostic key for limit-denial events.
    ///
    /// Keys are stable machine-readable identifiers intended for metrics,
    /// dashboards, and support triage. They are redacted for the same reason as
    /// [`AuditEvent::limit_denial_shape`].
    pub fn limit_denial_diagnostic_key(&self) -> Option<&'static str> {
        self.limit_denial_kind()
            .map(LimitDenialKind::diagnostic_key)
    }

    fn limit_denial_kind(&self) -> Option<LimitDenialKind> {
        match self {
            AuditEvent::PreflightFailed {
                reason: PreflightFailure::ResourceLimitExceeded { reason },
                ..
            } => limit_denial_kind_from_resource_reason(reason),
            AuditEvent::CapabilityCallExecuted {
                denial_category: Some(category),
                ..
            } => limit_denial_kind_from_category(category),
            _ => None,
        }
    }

    /// Return a stable, redacted runtime issue descriptor for this event.
    ///
    /// This is the single-event form used by
    /// [`runtime_issue_descriptors_for_events`] and
    /// [`AuditLog::runtime_issue_descriptors`]. The descriptor is redacted and
    /// stable: it carries only the coarse issue axis, diagnostic key, and shape.
    pub fn runtime_issue_descriptor(&self) -> Option<RuntimeIssueDescriptor> {
        match self {
            AuditEvent::PreflightFailed { reason, .. } => {
                runtime_issue_descriptor_from_preflight_failure(reason)
            }
            AuditEvent::CapabilityCallExecuted {
                denial_category: Some(category),
                ..
            } => runtime_issue_descriptor_from_denial_category(category),
            _ => None,
        }
    }

    /// Return a stable, redacted shape descriptor for secret access events.
    ///
    /// The descriptor deliberately ignores the concrete secret ID in
    /// `secret.read:<id>` capability names. Use this helper for summaries,
    /// metrics, and audit views that must not expose secret names while still
    /// distinguishing secret-access traffic from other capability calls.
    pub fn secret_access_shape(&self) -> Option<&'static str> {
        match self {
            AuditEvent::CapabilityCallExecuted { capability, .. } => {
                match capability.as_str().strip_prefix("secret.read:") {
                    Some("") => Some(SECRET_ACCESS_SHAPE_MALFORMED),
                    Some(_) => Some(SECRET_ACCESS_SHAPE_READ),
                    None => None,
                }
            }
            _ => None,
        }
    }
}

// ── AuditLog ─────────────────────────────────────────────────────────────

/// In-memory ordered sequence of [`AuditEvent`]s.
///
/// Events are appended in call order and are never removed or reordered.
#[derive(Clone, Debug, Default)]
pub struct AuditLog(Vec<AuditEvent>);

impl AuditLog {
    /// Create an empty log.
    pub fn new() -> Self {
        AuditLog(Vec::new())
    }

    /// Append an event (called once per preflight).
    pub fn push(&mut self, event: AuditEvent) {
        self.0.push(event);
    }

    /// Read-only ordered view of all events.
    pub fn events(&self) -> &[AuditEvent] {
        &self.0
    }

    /// Total number of events recorded.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// `true` if no events have been recorded yet.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Return deterministic, de-duplicated runtime issue descriptors for this log.
    ///
    /// Output order is canonical and does not depend on event insertion order:
    /// timeout, step, memory, capability, then resource-policy descriptors.
    pub fn runtime_issue_descriptors(&self) -> Vec<RuntimeIssueDescriptor> {
        runtime_issue_descriptors_for_events(self.events())
    }
}

/// Return deterministic, de-duplicated runtime issue descriptors for a batch of events.
///
/// The batch helper is intentionally redacted. It ignores successful events and
/// non-policy/non-limit failures, de-duplicates repeated issue classes, and
/// returns descriptors in canonical order for stable validation snapshots.
pub fn runtime_issue_descriptors_for_events<'a>(
    events: impl IntoIterator<Item = &'a AuditEvent>,
) -> Vec<RuntimeIssueDescriptor> {
    use std::collections::BTreeSet;

    events
        .into_iter()
        .filter_map(AuditEvent::runtime_issue_descriptor)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_audit_categories_are_stable_machine_readable_values() {
        let categories = [
            SECRET_AUDIT_CATEGORY_NOT_FOUND,
            SECRET_AUDIT_CATEGORY_PROVIDER_UNAVAILABLE,
            SECRET_AUDIT_CATEGORY_UNSUPPORTED_OPERATION,
            SECRET_AUDIT_CATEGORY_MALFORMED_CAPABILITY,
        ];

        for category in categories {
            assert!(
                category.starts_with("secret."),
                "category `{category}` must stay under secret namespace"
            );
            assert!(
                category
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'.' || byte == b'_'),
                "category `{category}` must stay machine readable"
            );
            assert!(
                !category.contains("ApiKey") && !category.contains("password"),
                "category `{category}` must not carry secret names"
            );
        }
    }

    #[test]
    fn secret_access_shape_redacts_secret_names() {
        let event = AuditEvent::CapabilityCallExecuted {
            capability: CapabilityId::new("secret.read:ProductionDbPassword"),
            operation: "read".to_string(),
            handler_name: "secret.read".to_string(),
            succeeded: false,
            duration_us: 1,
            timestamp: 1,
            profile: None,
            module: None,
            function: None,
            input_hash: None,
            output_hash: None,
            trace_id: None,
            verification_report_hash: None,
            trace_context: None,
            denial_category: Some(SECRET_AUDIT_CATEGORY_NOT_FOUND.to_string()),
        };

        assert_eq!(event.secret_access_shape(), Some(SECRET_ACCESS_SHAPE_READ));
        let shape = event.secret_access_shape().expect("secret shape");
        assert!(!shape.contains("ProductionDbPassword"));
    }

    #[test]
    fn secret_access_shape_marks_empty_secret_suffix_as_malformed() {
        let event = AuditEvent::CapabilityCallExecuted {
            capability: CapabilityId::new("secret.read:"),
            operation: "read".to_string(),
            handler_name: "secret.read".to_string(),
            succeeded: false,
            duration_us: 1,
            timestamp: 1,
            profile: None,
            module: None,
            function: None,
            input_hash: None,
            output_hash: None,
            trace_id: None,
            verification_report_hash: None,
            trace_context: None,
            denial_category: Some(SECRET_AUDIT_CATEGORY_MALFORMED_CAPABILITY.to_string()),
        };

        assert_eq!(
            event.secret_access_shape(),
            Some(SECRET_ACCESS_SHAPE_MALFORMED)
        );
    }

    #[test]
    fn runtime_denial_categories_are_stable_machine_readable_values() {
        let categories = [
            DENIAL_CATEGORY_CAPABILITY_NOT_GRANTED,
            DENIAL_CATEGORY_CAPABILITY_REVOKED,
            DENIAL_CATEGORY_HANDLER_NOT_BOUND,
            DENIAL_CATEGORY_LIMIT_PAYLOAD_SIZE,
            DENIAL_CATEGORY_LIMIT_MEMORY,
            DENIAL_CATEGORY_LIMIT_FUEL,
            DENIAL_CATEGORY_LIMIT_TIMEOUT,
            DENIAL_CATEGORY_LIMIT_MAX_CAPABILITY_CALLS,
            DENIAL_CATEGORY_LIMIT_RATE,
            DENIAL_CATEGORY_LIMIT_CONCURRENCY,
            DENIAL_CATEGORY_LIMIT_RECURSION_DEPTH,
            DENIAL_CATEGORY_LIMIT_OUTPUT_SIZE,
            DENIAL_CATEGORY_SCHEMA_INPUT,
            DENIAL_CATEGORY_SCHEMA_OUTPUT,
            DENIAL_CATEGORY_PAYLOAD_DECODE,
        ];

        for category in categories {
            assert!(
                category.contains('.'),
                "category `{category}` must include a namespace"
            );
            assert!(
                category
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'.' || byte == b'_'),
                "category `{category}` must stay machine readable"
            );
            assert_eq!(
                denial_category(category),
                Some(category.to_string()),
                "helper must preserve the stable category text"
            );
        }
    }

    #[test]
    fn limit_denial_shape_and_key_redact_resource_reason_details() {
        let event = AuditEvent::PreflightFailed {
            profile_name: "prod".to_string(),
            denied: vec![],
            reason: PreflightFailure::ResourceLimitExceeded {
                reason: "memory growth denied by resource limiter for tenant-prod".to_string(),
            },
        };

        assert_eq!(event.limit_denial_shape(), Some(LIMIT_DENIAL_SHAPE_MEMORY));
        assert_eq!(
            event.limit_denial_diagnostic_key(),
            Some(LIMIT_DENIAL_DIAGNOSTIC_KEY_MEMORY)
        );
        assert!(!event.limit_denial_shape().unwrap().contains("tenant-prod"));
    }

    #[test]
    fn limit_denial_shape_and_key_cover_dispatch_call_depth() {
        let event = AuditEvent::CapabilityCallExecuted {
            capability: CapabilityId::new("ops.internal_sensitive_capability"),
            operation: "private-operation".to_string(),
            handler_name: "none".to_string(),
            succeeded: false,
            duration_us: 1,
            timestamp: 1,
            profile: None,
            module: None,
            function: None,
            input_hash: None,
            output_hash: None,
            trace_id: None,
            verification_report_hash: None,
            trace_context: None,
            denial_category: Some(DENIAL_CATEGORY_LIMIT_RECURSION_DEPTH.to_string()),
        };

        assert_eq!(
            event.limit_denial_shape(),
            Some(LIMIT_DENIAL_SHAPE_CALL_DEPTH)
        );
        assert_eq!(
            event.limit_denial_diagnostic_key(),
            Some(LIMIT_DENIAL_DIAGNOSTIC_KEY_CALL_DEPTH)
        );
        assert!(!event.limit_denial_shape().unwrap().contains("ops.internal"));
    }

    #[test]
    fn limit_denial_shape_and_key_cover_fuel_and_time() {
        let fuel_event = AuditEvent::PreflightFailed {
            profile_name: "prod".to_string(),
            denied: vec![],
            reason: PreflightFailure::ResourceLimitExceeded {
                reason: "fuel limit exceeded after private loop".to_string(),
            },
        };
        let timeout_event = AuditEvent::PreflightFailed {
            profile_name: "prod".to_string(),
            denied: vec![],
            reason: PreflightFailure::ResourceLimitExceeded {
                reason: "deadline exceeded while running private module".to_string(),
            },
        };

        assert_eq!(
            fuel_event.limit_denial_shape(),
            Some(LIMIT_DENIAL_SHAPE_FUEL)
        );
        assert_eq!(
            fuel_event.limit_denial_diagnostic_key(),
            Some(LIMIT_DENIAL_DIAGNOSTIC_KEY_FUEL)
        );
        assert_eq!(
            timeout_event.limit_denial_shape(),
            Some(LIMIT_DENIAL_SHAPE_TIME)
        );
        assert_eq!(
            timeout_event.limit_denial_diagnostic_key(),
            Some(LIMIT_DENIAL_DIAGNOSTIC_KEY_TIME)
        );
    }

    #[test]
    fn replay_mismatch_categories_are_stable_machine_readable_values() {
        let categories = [
            REPLAY_MISMATCH_MISSING_RECORDING,
            REPLAY_MISMATCH_HASH_MISMATCH,
        ];

        for category in categories {
            assert!(
                category.starts_with("replay."),
                "category `{category}` must stay under replay namespace"
            );
            assert!(
                category
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'.' || byte == b'_'),
                "category `{category}` must stay machine readable"
            );
        }
    }

    #[test]
    fn transaction_lifecycle_categories_are_stable_machine_readable_values() {
        let categories = [
            TRANSACTION_CATEGORY_COMMITTED,
            TRANSACTION_CATEGORY_ROLLED_BACK,
            TRANSACTION_CATEGORY_COMMIT_ALREADY_COMMITTED,
            TRANSACTION_CATEGORY_COMMIT_AFTER_ROLLBACK,
            TRANSACTION_CATEGORY_ROLLBACK_AFTER_COMMIT,
            TRANSACTION_CATEGORY_ROLLBACK_ALREADY_ROLLED_BACK,
        ];

        for category in categories {
            assert!(
                category.starts_with("transaction."),
                "category `{category}` must stay under transaction namespace"
            );
            assert!(
                category
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'.' || byte == b'_'),
                "category `{category}` must stay machine readable"
            );
        }
    }
}
