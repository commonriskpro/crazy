// ── ail-runtime::audit ───────────────────────────────────────────────────
//
// Audit event types and in-memory ordered log.
//
// Spec invariants:
//   - Exactly one event is appended per preflight call.
//   - Events are ordered by insertion (Vec<AuditEvent>).
//   - Payloads MUST NOT contain raw WASM bytes, user data, or secrets.
//   - Events MAY include hash digests and denied capability names.

use crate::error::PreflightFailure;
use crate::profile::CapabilityId;

// ── AuditEvent ────────────────────────────────────────────────────────────

/// A single preflight audit record.
///
/// One event is appended to [`AuditLog`] per `validate_and_instantiate` call.
/// Payloads are redacted: no raw WASM bytes, no user data, no secrets.
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
}

impl AuditEvent {
    /// `true` if this event represents a successful preflight.
    pub fn is_passed(&self) -> bool {
        matches!(self, AuditEvent::PreflightPassed { .. })
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
