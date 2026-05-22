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

// ── RuntimeCheck ─────────────────────────────────────────────────────────

/// A single materialized runtime check recorded in the execution report.
///
/// Per runtime.md §"Runtime checks":
/// > Runtime host ejecuta checks materializados:
/// > decoder validations, refinement checks, capability response validation,
/// > range/bounds checks, boundary schema validation
///
/// `runtime_checked only counts if check exists in verified artifact hash.`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCheck {
    /// Name of the check (e.g. `"decoder_validation"`, `"boundary_schema"`).
    pub check_name: String,
    /// The capability this check is associated with, if applicable.
    pub capability: Option<CapabilityId>,
    /// Whether the check passed, failed, or was skipped.
    pub result: RuntimeCheckResult,
}

/// Outcome of a single materialized runtime check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeCheckResult {
    /// Check executed and passed.
    Passed,
    /// Check executed and failed.
    Failed,
    /// Check was not applicable and was skipped.
    Skipped,
}

// ── LimitSnapshot ─────────────────────────────────────────────────────────

/// Snapshot of one resource limit at the end of execution.
///
/// Per runtime.md §"Limits and sandboxing":
/// > timeout, memory limit, fuel/instruction limit, max capability calls, etc.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LimitSnapshot {
    /// Name of the limit (e.g. `"timeout"`, `"memory"`, `"max_capability_calls"`).
    pub limit_name: String,
    /// Configured value (e.g. `"5s"`, `"128MiB"`).  `None` if unconfigured.
    pub configured: Option<String>,
    /// Actual usage recorded during execution.  `None` if not tracked.
    pub used: Option<String>,
}

// ── RuntimeReport ─────────────────────────────────────────────────────────

/// Structured execution report emitted after a module run.
///
/// Produced by [`RuntimeHost::emit_report`](crate::host::RuntimeHost::emit_report)
/// after instantiation and any capability calls have completed.
///
/// The report records all fields described in docs/runtime.md §"Runtime report":
/// - `id` — caller-supplied trace/request ID
/// - `profile_name` — active profile name
/// - `module_name` — module identity (from manifest)
/// - `module_hash` — BLAKE3 hex of the executed WASM
/// - `verification_report_hash` — hash from the active profile
/// - `status` — execution outcome
/// - `capability_summaries` — per-capability call statistics
/// - `runtime_checks` — materialized runtime check results
/// - `limits` — resource limit snapshots
/// - `audit_log_hash` — BLAKE3 hash of the audit log contents
#[derive(Clone, Debug)]
pub struct RuntimeReport {
    id: String,
    profile_name: String,
    module_name: String,
    module_hash: String,
    verification_report_hash: String,
    status: RuntimeReportStatus,
    capability_summaries: Vec<CapabilityCallSummary>,
    runtime_checks: Vec<RuntimeCheck>,
    limits: Vec<LimitSnapshot>,
    audit_log_hash: Option<String>,
}

impl RuntimeReport {
    /// Construct a new `RuntimeReport`.
    ///
    /// `id` — caller-supplied identifier (e.g. trace ID).
    /// `profile_name` — name of the active runtime profile.
    /// `module_name` — logical module name (from capability manifest or profile).
    /// `module_hash` — BLAKE3 hex of the executed WASM module.
    /// `status` — execution outcome.
    pub fn new(
        id: String,
        profile_name: String,
        module_name: String,
        module_hash: String,
        status: RuntimeReportStatus,
    ) -> Self {
        RuntimeReport {
            id,
            profile_name,
            module_name,
            module_hash,
            verification_report_hash: String::new(),
            status,
            capability_summaries: Vec::new(),
            runtime_checks: Vec::new(),
            limits: Vec::new(),
            audit_log_hash: None,
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

    /// Logical module name (from the capability manifest).
    pub fn module_name(&self) -> &str {
        &self.module_name
    }

    /// BLAKE3 hex digest of the executed WASM module.
    pub fn module_hash(&self) -> &str {
        &self.module_hash
    }

    /// BLAKE3 hex digest of the verification report referenced by the profile.
    pub fn verification_report_hash(&self) -> &str {
        &self.verification_report_hash
    }

    /// Execution outcome.
    pub fn status(&self) -> &RuntimeReportStatus {
        &self.status
    }

    /// Per-capability call summaries.
    pub fn capability_summaries(&self) -> &[CapabilityCallSummary] {
        &self.capability_summaries
    }

    /// Materialized runtime check results.
    pub fn runtime_checks(&self) -> &[RuntimeCheck] {
        &self.runtime_checks
    }

    /// Resource limit snapshots from this execution.
    pub fn limits(&self) -> &[LimitSnapshot] {
        &self.limits
    }

    /// BLAKE3 hex digest of the audit log, if available.
    pub fn audit_log_hash(&self) -> Option<&str> {
        self.audit_log_hash.as_deref()
    }

    // ── builder methods ───────────────────────────────────────────────────

    /// Set the `verification_report_hash` (builder style).
    pub fn with_verification_report_hash(mut self, hash: String) -> Self {
        self.verification_report_hash = hash;
        self
    }

    /// Attach per-capability summaries (called by `RuntimeHost::emit_report`).
    pub(crate) fn with_summaries(mut self, summaries: Vec<CapabilityCallSummary>) -> Self {
        self.capability_summaries = summaries;
        self
    }

    /// Attach materialized runtime check results.
    pub fn with_runtime_checks(mut self, checks: Vec<RuntimeCheck>) -> Self {
        self.runtime_checks = checks;
        self
    }

    /// Attach resource limit snapshots.
    pub fn with_limits(mut self, limits: Vec<LimitSnapshot>) -> Self {
        self.limits = limits;
        self
    }

    /// Set the audit log hash.
    pub fn with_audit_log_hash(mut self, hash: String) -> Self {
        self.audit_log_hash = Some(hash);
        self
    }
}
