// ── ail-runtime::abi ─────────────────────────────────────────────────────
//
// Host ABI types.
//
// `HostCallId` identifies a WASM → host call.
// `HostError` is a typed enum with 11 discriminated variants matching the
// error taxonomy in `docs/runtime.md §Error model`.
// `HostResult<T>` is the standard return type for host-call handlers.

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

/// Typed error returned by a host-call handler.
///
/// Corresponds to the `HostError` taxonomy in `docs/runtime.md §Error model`.
/// Each variant carries a `String` payload with a human-readable description.
///
/// `Custom` is provided for extensibility — use it for errors that do not fit
/// any of the 11 standard categories.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostError {
    /// The requested capability was not granted to this module/profile.
    CapabilityDenied(String),
    /// The capability is granted but no handler has been bound to it.
    HandlerNotBound(String),
    /// The input payload could not be decoded against the capability schema.
    PayloadDecodeError(String),
    /// An argument could not be encoded into the capability payload format.
    PayloadEncodeError(String),
    /// The call violated a declared contract (e.g. idempotency, pre/postcondition).
    ContractViolation(String),
    /// The capability call exceeded its time budget.
    Timeout(String),
    /// A resource limit (memory, fuel, call count, payload size, …) was exceeded.
    LimitExceeded(String),
    /// The handler exists but is currently unavailable (e.g. circuit-breaker open).
    HandlerUnavailable(String),
    /// The host/WASM boundary protocol failed (encode/decode mismatch).
    BoundaryFailure(String),
    /// The audit subsystem failed to record this call.
    AuditFailure(String),
    /// The capability manifest or verification report hash did not match.
    ManifestMismatch(String),
    /// A catch-all for errors that do not fit the standard categories.
    Custom(String),
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostError::CapabilityDenied(msg) => write!(f, "capability denied: {msg}"),
            HostError::HandlerNotBound(msg) => write!(f, "handler not bound: {msg}"),
            HostError::PayloadDecodeError(msg) => write!(f, "payload decode error: {msg}"),
            HostError::PayloadEncodeError(msg) => write!(f, "payload encode error: {msg}"),
            HostError::ContractViolation(msg) => write!(f, "contract violation: {msg}"),
            HostError::Timeout(msg) => write!(f, "timeout: {msg}"),
            HostError::LimitExceeded(msg) => write!(f, "limit exceeded: {msg}"),
            HostError::HandlerUnavailable(msg) => write!(f, "handler unavailable: {msg}"),
            HostError::BoundaryFailure(msg) => write!(f, "boundary failure: {msg}"),
            HostError::AuditFailure(msg) => write!(f, "audit failure: {msg}"),
            HostError::ManifestMismatch(msg) => write!(f, "manifest mismatch: {msg}"),
            HostError::Custom(msg) => write!(f, "host error: {msg}"),
        }
    }
}

impl std::error::Error for HostError {}

// ── HostResult<T> ─────────────────────────────────────────────────────────

/// Return type for host-call handlers.
pub type HostResult<T> = Result<T, HostError>;
