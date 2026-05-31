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

pub(crate) fn denial_category(category: &'static str) -> Option<String> {
    Some(category.to_string())
}

// ── Stable replay mismatch categories ────────────────────────────────────

/// Replay/audit category for calls that have no recorded capability response.
pub const REPLAY_MISMATCH_MISSING_RECORDING: &str = "replay.missing_recording";
/// Replay/audit category for recorded responses whose output hash no longer matches.
pub const REPLAY_MISMATCH_HASH_MISMATCH: &str = "replay.hash_mismatch";

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_denial_categories_are_stable_machine_readable_values() {
        let categories = [
            DENIAL_CATEGORY_CAPABILITY_NOT_GRANTED,
            DENIAL_CATEGORY_CAPABILITY_REVOKED,
            DENIAL_CATEGORY_HANDLER_NOT_BOUND,
            DENIAL_CATEGORY_LIMIT_PAYLOAD_SIZE,
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
