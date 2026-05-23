// ── ail-remote::policy ─────────────────────────────────────────────────────
//
// Transport-agnostic policy primitives for deciding which remote signers are
// allowed to submit signed changesets.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::AgentIdentity;

/// Trust tier assigned by the local authority to an allowed remote signer.
///
/// This is policy metadata for callers and audit output. The current coordinator
/// submit gate is binary: a signer is either present in the allowlist or rejected.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignerTrustTier {
    Primary,
    Trusted,
    External,
}

/// A remote signer explicitly allowed by local policy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedRemoteSigner {
    /// Raw Ed25519 public key bytes used as the stable signer identity.
    pub public_key: [u8; 32],
    /// Local trust tier for this signer.
    pub trust_tier: SignerTrustTier,
    /// Local display label for diagnostics. This is configured by policy, not
    /// trusted from a submitted [`AgentIdentity`] label.
    pub label: Option<String>,
}

impl TrustedRemoteSigner {
    pub fn new(public_key: [u8; 32], trust_tier: SignerTrustTier, label: Option<String>) -> Self {
        Self {
            public_key,
            trust_tier,
            label,
        }
    }

    pub fn from_identity(
        identity: &AgentIdentity,
        trust_tier: SignerTrustTier,
        label: Option<String>,
    ) -> Self {
        Self::new(identity.public_key, trust_tier, label)
    }
}

/// Allowlist policy for remote changeset signers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteSignerPolicy {
    pub allowed_signers: Vec<TrustedRemoteSigner>,
}

impl RemoteSignerPolicy {
    pub fn deny_all() -> Self {
        Self::default()
    }

    pub fn from_allowed_signers(allowed_signers: Vec<TrustedRemoteSigner>) -> Self {
        Self { allowed_signers }
    }

    pub fn check_identity(
        &self,
        identity: &AgentIdentity,
    ) -> Result<&TrustedRemoteSigner, RemoteSignerRejection> {
        self.allowed_signers
            .iter()
            .find(|signer| signer.public_key == identity.public_key)
            .ok_or_else(|| RemoteSignerRejection {
                public_key: identity.public_key,
                label: identity.label.clone(),
                reason: RemoteSignerRejectionReason::SignerNotAllowed,
            })
    }
}

/// Machine-readable reason a remote signer was rejected by policy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemoteSignerRejectionReason {
    SignerNotAllowed,
}

/// Details returned when a validly signed envelope comes from a disallowed signer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteSignerRejection {
    pub public_key: [u8; 32],
    pub label: Option<String>,
    pub reason: RemoteSignerRejectionReason,
}

impl fmt::Display for RemoteSignerRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.reason {
            RemoteSignerRejectionReason::SignerNotAllowed => {
                write!(f, "remote signer is not allowed by policy")
            }
        }
    }
}

impl std::error::Error for RemoteSignerRejection {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgentKeypair;

    #[test]
    fn allowed_identity_passes_policy() {
        let keypair = AgentKeypair::generate();
        let identity = keypair.identity();
        let policy =
            RemoteSignerPolicy::from_allowed_signers(vec![TrustedRemoteSigner::from_identity(
                &identity,
                SignerTrustTier::Trusted,
                Some("remote-a".to_string()),
            )]);

        let signer = policy
            .check_identity(&identity)
            .expect("allowed signer must pass policy");

        assert_eq!(signer.trust_tier, SignerTrustTier::Trusted);
        assert_eq!(signer.label.as_deref(), Some("remote-a"));
    }

    #[test]
    fn unknown_identity_returns_rejection_reason() {
        let keypair = AgentKeypair::generate();
        let mut identity = keypair.identity();
        identity.label = Some("untrusted-label".to_string());
        let policy = RemoteSignerPolicy::deny_all();

        let rejection = policy
            .check_identity(&identity)
            .expect_err("unknown signer must be rejected");

        assert_eq!(rejection.public_key, identity.public_key);
        assert_eq!(rejection.label.as_deref(), Some("untrusted-label"));
        assert_eq!(
            rejection.reason,
            RemoteSignerRejectionReason::SignerNotAllowed
        );
    }
}
