// ── ail-remote::policy ─────────────────────────────────────────────────────
//
// Transport-agnostic policy primitives for deciding which remote signers are
// allowed to submit signed changesets.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::AgentIdentity;

/// Serializable project-level remote collaboration configuration.
///
/// This DTO is transport- and CLI-agnostic: callers can load it from JSON, CBOR,
/// or another serde-backed format, then validate it before building runtime
/// policy. Signer authority comes only from `allowed_signers`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteConfig {
    /// Explicit remote signers allowed to submit signed changesets.
    pub allowed_signers: Vec<RemoteSignerConfig>,
    /// Named remote endpoints known to the project. These are connection hints,
    /// not authority grants.
    pub remotes: Vec<RemoteEndpointConfig>,
}

impl RemoteConfig {
    /// Validate the config without constructing a runtime policy.
    ///
    /// # Errors
    ///
    /// Returns [`RemoteConfigError`] for malformed signer keys, duplicate signer
    /// keys, or empty/duplicate remote endpoint names.
    pub fn validate(&self) -> Result<(), RemoteConfigError> {
        let mut signer_keys = BTreeSet::new();
        for signer in &self.allowed_signers {
            let public_key = parse_remote_public_key_hex(&signer.public_key)?;
            if !signer_keys.insert(public_key) {
                return Err(RemoteConfigError::DuplicateAllowedSigner {
                    public_key: signer.public_key.clone(),
                });
            }
        }

        let mut remote_names = BTreeSet::new();
        for remote in &self.remotes {
            if remote.name.trim().is_empty() {
                return Err(RemoteConfigError::EmptyRemoteName);
            }
            if !remote_names.insert(remote.name.as_str()) {
                return Err(RemoteConfigError::DuplicateRemoteName {
                    name: remote.name.clone(),
                });
            }
        }

        Ok(())
    }

    /// Convert validated config into the runtime signer allowlist policy.
    ///
    /// An empty `allowed_signers` list is valid and produces `deny_all()`.
    pub fn to_signer_policy(&self) -> Result<RemoteSignerPolicy, RemoteConfigError> {
        self.validate()?;

        let allowed_signers = self
            .allowed_signers
            .iter()
            .map(RemoteSignerConfig::to_trusted_signer)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(RemoteSignerPolicy::from_allowed_signers(allowed_signers))
    }
}

/// Serializable signer entry used by [`RemoteConfig`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteSignerConfig {
    /// Hex-encoded 32-byte Ed25519 public key.
    pub public_key: String,
    /// Local trust metadata for diagnostics/audit.
    pub trust_tier: SignerTrustTier,
    /// Local display label. This is not trusted from submitted identities.
    pub label: Option<String>,
}

impl RemoteSignerConfig {
    pub fn to_trusted_signer(&self) -> Result<TrustedRemoteSigner, RemoteConfigError> {
        Ok(TrustedRemoteSigner::new(
            parse_remote_public_key_hex(&self.public_key)?,
            self.trust_tier.clone(),
            self.label.clone(),
        ))
    }
}

/// Serializable remote endpoint metadata used by [`RemoteConfig`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteEndpointConfig {
    /// Stable project-local name, e.g. `origin`.
    pub name: String,
    /// Transport-specific endpoint string. It is intentionally not parsed here.
    pub endpoint: String,
}

/// Error returned when remote config cannot be converted into policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteConfigError {
    InvalidPublicKey { public_key: String, reason: String },
    DuplicateAllowedSigner { public_key: String },
    EmptyRemoteName,
    DuplicateRemoteName { name: String },
}

impl fmt::Display for RemoteConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RemoteConfigError::InvalidPublicKey { reason, .. } => {
                write!(f, "remote config public key is invalid: {reason}")
            }
            RemoteConfigError::DuplicateAllowedSigner { .. } => {
                write!(f, "remote config contains duplicate allowed signer")
            }
            RemoteConfigError::EmptyRemoteName => write!(f, "remote config remote name is empty"),
            RemoteConfigError::DuplicateRemoteName { .. } => {
                write!(f, "remote config contains duplicate remote name")
            }
        }
    }
}

impl std::error::Error for RemoteConfigError {}

/// Parse a hex-encoded 32-byte Ed25519 public key used by remote config.
pub fn parse_remote_public_key_hex(public_key: &str) -> Result<[u8; 32], RemoteConfigError> {
    let bytes = public_key.as_bytes();
    if bytes.len() != 64 {
        return Err(RemoteConfigError::InvalidPublicKey {
            public_key: public_key.to_string(),
            reason: "expected 64 hex characters for a 32-byte Ed25519 public key".to_string(),
        });
    }

    let mut parsed = [0u8; 32];
    for (index, chunk) in bytes.chunks_exact(2).enumerate() {
        let high =
            decode_hex_nibble(chunk[0]).ok_or_else(|| RemoteConfigError::InvalidPublicKey {
                public_key: public_key.to_string(),
                reason: "public key contains a non-hex character".to_string(),
            })?;
        let low =
            decode_hex_nibble(chunk[1]).ok_or_else(|| RemoteConfigError::InvalidPublicKey {
                public_key: public_key.to_string(),
                reason: "public key contains a non-hex character".to_string(),
            })?;
        parsed[index] = (high << 4) | low;
    }

    Ok(parsed)
}

fn decode_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

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
