// ── ail-runtime::report ───────────────────────────────────────────────────
//
// RuntimeReport — structured execution report emitted by the runtime (G29).
//
// Per runtime.md §"Runtime report":
//   runtime_report <id>
//   profile prod
//   module module.checkout
//   verification_report hash=ver_abc123
//   status completed | failed | denied | timeout | limit_exceeded
//   capability_calls ... end
//   runtime_checks ... end
//   limits ... end
//   audit_log hash=audit_123
//   end
//
// `RuntimeHost::emit_report(status, id)` aggregates data from the current
// profile and audit log to produce a `RuntimeReport`.

use crate::profile::CapabilityId;

// ── RuntimeReportStatus ───────────────────────────────────────────────────

/// Execution outcome captured in a [`RuntimeReport`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeReportStatus {
    /// Module executed to completion without errors.
    Completed,
    /// Module returned an error or a handler failed.
    Failed,
    /// A capability was denied at runtime (not granted).
    Denied,
    /// Execution was aborted due to a timeout.
    Timeout,
    /// Execution was aborted because a resource limit was exceeded.
    LimitExceeded,
}

impl std::fmt::Display for RuntimeReportStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeReportStatus::Completed => write!(f, "completed"),
            RuntimeReportStatus::Failed => write!(f, "failed"),
            RuntimeReportStatus::Denied => write!(f, "denied"),
            RuntimeReportStatus::Timeout => write!(f, "timeout"),
            RuntimeReportStatus::LimitExceeded => write!(f, "limit_exceeded"),
        }
    }
}

// ── CapabilityCallSummary ─────────────────────────────────────────────────

/// Aggregated statistics for one capability across all calls in an execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityCallSummary {
    /// The capability that was called.
    pub capability: CapabilityId,
    /// Total number of calls dispatched.
    pub total_calls: u32,
    /// Calls that returned `Ok(_)`.
    pub succeeded: u32,
    /// Calls that returned `Err(_)`.
    pub failed: u32,
}

// ── RuntimeReport ─────────────────────────────────────────────────────────

/// Structured execution report emitted after a module run.
///
/// Produced by [`RuntimeHost::emit_report`](crate::host::RuntimeHost::emit_report)
/// after instantiation and any capability calls have completed.
///
/// The report records:
/// - A caller-supplied `id` (e.g. a trace ID or request ID).
/// - The active profile name and module hash.
/// - The final execution status.
/// - Per-capability call summaries derived from the audit log.
#[derive(Clone, Debug)]
pub struct RuntimeReport {
    id: String,
    profile_name: String,
    module_hash: String,
    status: RuntimeReportStatus,
    capability_summaries: Vec<CapabilityCallSummary>,
}

impl RuntimeReport {
    /// Construct a new `RuntimeReport`.
    ///
    /// `id` — caller-supplied identifier (e.g. trace ID).
    /// `profile_name` — name of the active runtime profile.
    /// `module_hash` — BLAKE3 hex of the executed WASM module.
    /// `status` — execution outcome.
    pub fn new(
        id: String,
        profile_name: String,
        module_hash: String,
        status: RuntimeReportStatus,
    ) -> Self {
        RuntimeReport {
            id,
            profile_name,
            module_hash,
            status,
            capability_summaries: Vec::new(),
        }
    }

    /// Caller-supplied report identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Name of the runtime profile that was active during execution.
    pub fn profile_name(&self) -> &str {
        &self.profile_name
    }

    /// BLAKE3 hex digest of the executed WASM module.
    pub fn module_hash(&self) -> &str {
        &self.module_hash
    }

    /// Execution outcome.
    pub fn status(&self) -> &RuntimeReportStatus {
        &self.status
    }

    /// Per-capability call summaries.
    pub fn capability_summaries(&self) -> &[CapabilityCallSummary] {
        &self.capability_summaries
    }

    /// Attach per-capability summaries (called by `RuntimeHost::emit_report`).
    pub(crate) fn with_summaries(mut self, summaries: Vec<CapabilityCallSummary>) -> Self {
        self.capability_summaries = summaries;
        self
    }
}
