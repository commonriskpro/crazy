// ── ail-remote::identity ──────────────────────────────────────────────────
//
// Agent identity and keypair primitives for Ed25519 signing.
//
// # Design decisions
//
// - `AgentIdentity` stores the raw 32-byte Ed25519 public key as `[u8; 32]`.
//   This is the canonical wire format — deterministic CBOR serialization is
//   guaranteed because `[u8; N]` serialises as a fixed-length byte sequence.
// - `AgentKeypair` wraps `ed25519_dalek::SigningKey` and intentionally does NOT
//   expose the secret key bytes (`secret` field is private).
// - Signing produces a 64-byte signature as `[u8; 64]` — the Ed25519 signature
//   raw bytes, not a DER encoding.
// - `verify_bytes` on `AgentIdentity` rebuilds the `VerifyingKey` on each call;
//   key construction is cheap and avoids caching mutable state.
//
// # Workspace lint
//
// No `unsafe` blocks appear in this module.  `ed25519-dalek` uses `unsafe`
// internally, which is permitted by the workspace `deny(unsafe_code)` lint
// because it applies only to our own code.

use std::fmt;

use ed25519_dalek::{Signer, Verifier};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

// ── sig_serde: serialize [u8;64] as bytes (ciborium serde supports ≤32 natively) ──
//
// `ciborium`'s serde impl does not support fixed-length arrays larger than
// 32 elements via `Serialize`/`Deserialize` derive.  This module provides
// a custom `serde(with = …)` shim that serialises `[u8; 64]` as a CBOR
// byte string and deserialises it back.

pub(crate) mod sig_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(sig: &[u8; 64], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(sig)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 64], D::Error> {
        let bytes: Vec<u8> = Vec::<u8>::deserialize(d)?;
        bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("expected exactly 64 bytes for signature"))
    }
}

// ── SigningError ───────────────────────────────────────────────────────────

/// Error returned by signing and verification operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SigningError {
    /// The signature does not match the payload for the given identity.
    SignatureInvalid,
    /// Serialization of the signing payload failed.
    SerializationError(String),
}

impl fmt::Display for SigningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SigningError::SignatureInvalid => write!(f, "Ed25519 signature is invalid"),
            SigningError::SerializationError(msg) => {
                write!(f, "signing payload serialization failed: {msg}")
            }
        }
    }
}

impl std::error::Error for SigningError {}

// ── AgentIdentity ─────────────────────────────────────────────────────────

/// The verifiable identity of a remote agent: a 32-byte Ed25519 public key.
///
/// `label` is a human-readable name that is NOT authenticated — consumers
/// MUST NOT trust `label` for access-control decisions; only `public_key`
/// carries cryptographic authority.
///
/// Deterministic CBOR serialization is guaranteed: `[u8; 32]` serialises
/// as a fixed-length byte sequence, and `Option<String>` serialises
/// deterministically for fixed string contents.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentIdentity {
    /// Raw 32-byte Ed25519 public key bytes.
    pub public_key: [u8; 32],
    /// Optional human-readable label — not authenticated.
    pub label: Option<String>,
}

impl AgentIdentity {
    /// Verify that `sig` is a valid Ed25519 signature over `payload` by this identity.
    ///
    /// # Errors
    ///
    /// Returns `SigningError::SignatureInvalid` if the key bytes are malformed
    /// or if the signature does not match the payload.
    pub fn verify_bytes(&self, payload: &[u8], sig: &[u8; 64]) -> Result<(), SigningError> {
        let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&self.public_key)
            .map_err(|_| SigningError::SignatureInvalid)?;
        let signature = ed25519_dalek::Signature::from_bytes(sig);
        verifying_key
            .verify(payload, &signature)
            .map_err(|_| SigningError::SignatureInvalid)
    }
}

// ── AgentKeypair ──────────────────────────────────────────────────────────

/// An Ed25519 signing keypair for a remote agent.
///
/// The secret key is never exposed via public API.  Use `generate()` to
/// create a fresh keypair, `identity()` to derive the verifiable public
/// identity, and `sign_bytes()` to produce a 64-byte signature.
pub struct AgentKeypair {
    secret: ed25519_dalek::SigningKey,
}

impl AgentKeypair {
    /// Generate a fresh Ed25519 keypair using the OS random number generator.
    pub fn generate() -> Self {
        Self {
            secret: ed25519_dalek::SigningKey::generate(&mut OsRng),
        }
    }

    /// Derive the `AgentIdentity` (public key) for this keypair.
    pub fn identity(&self) -> AgentIdentity {
        AgentIdentity {
            public_key: self.secret.verifying_key().to_bytes(),
            label: None,
        }
    }

    /// Sign `payload` and return the 64-byte Ed25519 signature.
    pub fn sign_bytes(&self, payload: &[u8]) -> [u8; 64] {
        self.secret.sign(payload).to_bytes()
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── generate_produces_distinct_keypairs ───────────────────────────────
    // Task 3.1: `generate()` produces distinct keypairs.
    #[test]
    fn generate_produces_distinct_keypairs() {
        let kp_a = AgentKeypair::generate();
        let kp_b = AgentKeypair::generate();
        assert_ne!(
            kp_a.identity().public_key,
            kp_b.identity().public_key,
            "two independently generated keypairs must have distinct public keys"
        );
    }

    // ── sign_verify_roundtrip ─────────────────────────────────────────────
    // Task 3.1: `sign_bytes + verify_bytes` roundtrip succeeds.
    #[test]
    fn sign_verify_roundtrip() {
        let kp = AgentKeypair::generate();
        let identity = kp.identity();
        let payload = b"test signing payload";
        let sig = kp.sign_bytes(payload);
        identity
            .verify_bytes(payload, &sig)
            .expect("valid signature must verify successfully");
    }

    // ── wrong_key_returns_signature_invalid ───────────────────────────────
    // Task 3.1: wrong key returns `SignatureInvalid`.
    // TRIANGULATE: different keypair rejects the signature.
    #[test]
    fn wrong_key_returns_signature_invalid() {
        let kp_signer = AgentKeypair::generate();
        let kp_other = AgentKeypair::generate();
        let payload = b"some payload bytes";
        let sig = kp_signer.sign_bytes(payload);
        let result = kp_other.identity().verify_bytes(payload, &sig);
        assert_eq!(
            result,
            Err(SigningError::SignatureInvalid),
            "wrong key must return SignatureInvalid"
        );
    }
}
