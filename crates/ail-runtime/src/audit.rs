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

/// A single audit record.
///
/// Preflight events: one appended per `validate_and_instantiate` call.
/// Capability call events: one appended per `call_capability` call.
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

    /// A capability call was dispatched (or denied) via `call_capability`.
    ///
    /// Appended after every `call_capability` call regardless of outcome.
    /// Payload bytes are never included; only metadata is recorded.
    CapabilityCallExecuted {
        /// The capability that was requested.
        capability: CapabilityId,
        /// The specific operation within that capability.
        operation: String,
        /// Name of the handler that was dispatched to, or `"none"` if no
        /// handler was found or the call was denied before dispatch.
        handler_name: String,
        /// `true` if the handler returned `Ok(_)`, `false` otherwise.
        succeeded: bool,
        /// Wall-clock duration of the dispatch in microseconds.
        duration_us: u64,
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
