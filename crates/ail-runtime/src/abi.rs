// ── ail-runtime::abi ─────────────────────────────────────────────────────
//
// Host ABI types — DORMANT in Phase 8 PR 1.
//
// `HostCallId` and `HostResult<T>` are defined here so that they can be
// referenced in tests and documentation.  No linker imports or host-function
// handlers are registered in this phase; that wiring is deferred to a later
// runtime capability execution phase.

use crate::profile::CapabilityId;

// ── HostCallId ────────────────────────────────────────────────────────────

/// Discriminant for a WASM → host call.
///
/// Each variant identifies a class of host capability.  The linker wiring
/// that maps these IDs to actual host functions is out of scope for PR 1.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HostCallId {
    /// A call that exercises a named capability + operation pair.
    Capability {
        /// The capability being invoked.
        capability: CapabilityId,
        /// The specific operation within that capability (e.g. "read", "write").
        operation: String,
    },
}

// ── HostError ────────────────────────────────────────────────────────────

/// Error returned by a host-call handler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostError {
    /// Human-readable description of the host-side failure.
    pub message: String,
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "host error: {}", self.message)
    }
}

impl std::error::Error for HostError {}

// ── HostResult<T> ─────────────────────────────────────────────────────────

/// Return type for host-call handlers.
pub type HostResult<T> = Result<T, HostError>;
