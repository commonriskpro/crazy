// ── ail-remote::error ─────────────────────────────────────────────────────
//
// Top-level error type for remote collaboration operations.
//
// `RemoteError` is the error returned by `Coordinator::verify_remote_submission`.
// It is defined here (not in `ail-coordinator`) so that the leaf crate can
// declare the contract without depending on the coordinator.

use std::fmt;

use crate::policy::RemoteSignerRejection;

/// Error returned when a remote submission cannot be processed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteError {
    /// The Ed25519 signature on the submitted envelope failed verification.
    SignatureInvalid,
    /// The signature was valid, but local policy does not allow this signer.
    SignerRejected(RemoteSignerRejection),
    /// The coordinator itself reported an error after signature verification.
    CoordinatorFailed(String),
}

impl fmt::Display for RemoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RemoteError::SignatureInvalid => write!(f, "remote submission signature is invalid"),
            RemoteError::SignerRejected(rejection) => write!(f, "{rejection}"),
            RemoteError::CoordinatorFailed(reason) => {
                write!(f, "coordinator failed: {reason}")
            }
        }
    }
}

impl std::error::Error for RemoteError {}
