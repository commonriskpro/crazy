// ── ail-runtime::error ───────────────────────────────────────────────────
//
// Runtime error types: `RuntimeError` (top-level), `PreflightFailure` (cause).
//
// These types are structural — their job is to carry discriminants and
// payloads unambiguously. The TDD cycle for error types writes tests in
// profile_tests.rs and audit_tests.rs that exercise error variants as
// part of preflight behaviour, rather than here in isolation.

use ail_package::trust::TrustLevel;

use crate::profile::CapabilityId;

// ── PreflightFailure ─────────────────────────────────────────────────────

/// Reason a preflight check failed.
///
/// Preflight runs in order: (1) hash check → (2) capability check.
/// Failure at step 1 prevents step 2 from running.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreflightFailure {
    /// The BLAKE3 hash of the provided WASM bytes does not match the hash
    /// recorded in the [`RuntimeProfile`](crate::profile::RuntimeProfile).
    HashMismatch {
        /// Hash recorded in the profile.
        expected: String,
        /// Hash computed from the provided bytes.
        actual: String,
    },

    /// One or more capabilities required by the manifest are absent from
    /// the profile's grants list.
    CapabilityDenied {
        /// Capabilities that were required but not granted.
        denied: Vec<CapabilityId>,
    },

    /// Wasmtime rejected the WASM binary during structural validation.
    WasmValidationError(String),

    /// A package manifest's trust level does not meet the profile's minimum.
    ///
    /// Emitted during Stage 0 preflight when `profile.min_package_trust` is
    /// `Some(required)` and a declared package has `trust_level < required`.
    PackageTrustViolation {
        /// Package name as declared in the manifest.
        package: String,
        /// Minimum trust level required by the active profile.
        required: TrustLevel,
        /// Actual trust level of the package.
        actual: TrustLevel,
    },

    /// An `Unsafe` package has no explicit approval record in the profile.
    ///
    /// `TrustLevel::Unsafe` packages are unconditionally blocked unless the
    /// profile carries an explicit approval for that package.
    UnsafePackageNotApproved {
        /// Package name as declared in the manifest.
        package: String,
    },

    /// A `Verified` package did not provide consistent local verification evidence.
    PackageVerificationEvidenceInvalid {
        /// Package name as declared in the manifest.
        package: String,
        /// Human-readable evidence validation failure.
        reason: String,
    },

    /// A required capability has no bound handler.
    ///
    /// Emitted during preflight step 5 when `profile.require_handler_binding`
    /// is `true` and a granted capability has no registered handler.
    HandlerNotBound {
        /// The capability that is granted but not handled.
        capability: CapabilityId,
    },

    /// Execution was terminated because a resource limit was exceeded.
    ///
    /// Emitted when Wasmtime traps due to fuel exhaustion or memory growth
    /// being denied by the `StoreLimits` resource limiter.
    ResourceLimitExceeded {
        /// Human-readable description of the limit that was hit.
        reason: String,
    },
}

impl std::fmt::Display for PreflightFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PreflightFailure::HashMismatch { expected, actual } => {
                write!(f, "hash mismatch: expected {expected}, got {actual}")
            }
            PreflightFailure::CapabilityDenied { denied } => {
                let names: Vec<_> = denied.iter().map(|c| c.as_str()).collect();
                write!(f, "capability denied: {}", names.join(", "))
            }
            PreflightFailure::WasmValidationError(msg) => {
                write!(f, "wasm validation error: {msg}")
            }
            PreflightFailure::PackageTrustViolation {
                package,
                required,
                actual,
            } => {
                write!(
                    f,
                    "package trust violation: `{package}` has trust level `{actual}`, \
                     profile requires `{required}`"
                )
            }
            PreflightFailure::UnsafePackageNotApproved { package } => {
                write!(
                    f,
                    "unsafe package not approved: `{package}` has TrustLevel::Unsafe \
                     with no explicit approval in the profile"
                )
            }
            PreflightFailure::PackageVerificationEvidenceInvalid { package, reason } => {
                write!(
                    f,
                    "package verification evidence invalid: `{package}`: {reason}"
                )
            }
            PreflightFailure::HandlerNotBound { capability } => {
                write!(
                    f,
                    "handler not bound: capability `{}` is granted but no handler is registered",
                    capability.as_str()
                )
            }
            PreflightFailure::ResourceLimitExceeded { reason } => {
                write!(f, "resource limit exceeded: {reason}")
            }
        }
    }
}

// ── RuntimeError ─────────────────────────────────────────────────────────

/// Top-level error returned by [`RuntimeHost`](crate::host::RuntimeHost)
/// operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    /// Preflight did not pass; instantiation was not attempted.
    PreflightFailed(PreflightFailure),

    /// An internal encoding error (e.g. CBOR serialization) prevented
    /// hash computation.
    EncodingError(String),

    /// A capability call dispatched to a handler returned an error.
    ///
    /// Produced by [`RuntimeHost::call_capability`](crate::host::RuntimeHost::call_capability)
    /// when the handler returns `Err(HostError { .. })`.
    CapabilityCallFailed(crate::abi::HostError),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeError::PreflightFailed(cause) => {
                write!(f, "preflight failed: {cause}")
            }
            RuntimeError::EncodingError(msg) => {
                write!(f, "encoding error: {msg}")
            }
            RuntimeError::CapabilityCallFailed(err) => {
                write!(f, "capability call failed: {err}")
            }
        }
    }
}

impl std::error::Error for RuntimeError {}

/// Convenience alias used throughout the crate.
pub type RuntimeResult<T> = Result<T, RuntimeError>;

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use crate::profile::CapabilityId;

    // Structural test: PreflightFailure::HashMismatch carries both hashes.
    #[test]
    fn hash_mismatch_carries_expected_and_actual() {
        let e = PreflightFailure::HashMismatch {
            expected: "aaa".to_string(),
            actual: "bbb".to_string(),
        };
        match &e {
            PreflightFailure::HashMismatch { expected, actual } => {
                assert_eq!(expected, "aaa");
                assert_eq!(actual, "bbb");
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    // TRIANGULATE: CapabilityDenied carries the denied list.
    #[test]
    fn capability_denied_carries_denied_list() {
        let id = CapabilityId::new("NetworkEgress");
        let e = PreflightFailure::CapabilityDenied {
            denied: vec![id.clone()],
        };
        match &e {
            PreflightFailure::CapabilityDenied { denied } => {
                assert_eq!(denied.len(), 1);
                assert_eq!(denied[0], id);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    // TRIANGULATE: RuntimeError::PreflightFailed wraps the cause.
    #[test]
    fn runtime_error_wraps_preflight_failure() {
        let cause = PreflightFailure::WasmValidationError("bad magic".to_string());
        let err = RuntimeError::PreflightFailed(cause.clone());
        match &err {
            RuntimeError::PreflightFailed(inner) => assert_eq!(inner, &cause),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    // Display mentions the discriminant in each variant.
    #[test]
    fn display_identifies_each_variant() {
        let hash_err = PreflightFailure::HashMismatch {
            expected: "e".to_string(),
            actual: "a".to_string(),
        };
        assert!(hash_err.to_string().contains("hash mismatch"));

        let cap_err = PreflightFailure::CapabilityDenied {
            denied: vec![CapabilityId::new("X")],
        };
        assert!(cap_err.to_string().contains("capability denied"));

        let val_err = PreflightFailure::WasmValidationError("trap".to_string());
        assert!(val_err.to_string().contains("wasm validation error"));
    }
}
