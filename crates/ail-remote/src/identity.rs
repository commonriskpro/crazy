// ── ail-remote::identity ──────────────────────────────────────────────────
//
// Agent identity and keypair primitives for Ed25519 signing.
//
// # Design decisions
//
// - `AgentIdentity` stores the raw 32-byte Ed25519 public key as `[u8; 32]`.
//   This is the canonical wire format — deterministic CBOR serialization is
//   guaranteed because `[u8; N]` serialises as a fixed-length byte sequence.
// - `AgentKeypair` wraps `ed25519_dalek::SigningKey` and keeps the secret field
//   private. Plaintext export/import is intentionally limited to the explicit
//   `PlaintextDevSignerKeyMaterial` DTO for local development only.
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

/// Warning embedded in plaintext signer key material.
pub const PLAINTEXT_DEV_SIGNER_KEY_WARNING: &str =
    "plaintext Ed25519 signer key for local development only; not production secret storage";

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
    /// Signer key material could not be loaded safely.
    InvalidKeyMaterial(String),
}

impl fmt::Display for SigningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SigningError::SignatureInvalid => write!(f, "Ed25519 signature is invalid"),
            SigningError::SerializationError(msg) => {
                write!(f, "signing payload serialization failed: {msg}")
            }
            SigningError::InvalidKeyMaterial(msg) => {
                write!(f, "signer key material is invalid: {msg}")
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
/// The secret key field stays private. Use `generate()` to create a fresh
/// keypair, `identity()` to derive the verifiable public identity, and
/// `sign_bytes()` to produce a 64-byte signature. Explicit plaintext export is
/// limited to [`PlaintextDevSignerKeyMaterial`] for local development.
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

// ── PlaintextDevSignerKeyMaterial ─────────────────────────────────────────

/// Serializable plaintext Ed25519 signer key material for local development.
///
/// This DTO deliberately carries a warning and validates that the stored public
/// key matches the stored secret key when loading. It is not a production secret
/// storage format.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaintextDevSignerKeyMaterial {
    /// Format version for future migration.
    pub version: u8,
    /// Explicit warning that this is plaintext local-dev material.
    pub warning: String,
    /// Hex-encoded 32-byte Ed25519 secret key.
    pub secret_key_hex: String,
    /// Hex-encoded 32-byte Ed25519 public key expected from the secret key.
    pub public_key_hex: String,
    /// Optional local display label; not cryptographic authority.
    pub label: Option<String>,
}

impl PlaintextDevSignerKeyMaterial {
    pub const VERSION: u8 = 1;

    /// Create plaintext local-dev key material from an in-memory keypair.
    pub fn from_keypair(keypair: &AgentKeypair, label: Option<String>) -> Self {
        Self {
            version: Self::VERSION,
            warning: PLAINTEXT_DEV_SIGNER_KEY_WARNING.to_string(),
            secret_key_hex: encode_hex32(&keypair.secret.to_bytes()),
            public_key_hex: encode_hex32(&keypair.identity().public_key),
            label,
        }
    }

    /// Load an Ed25519 keypair, rejecting malformed material and key mismatch.
    pub fn to_keypair(&self) -> Result<AgentKeypair, SigningError> {
        if self.version != Self::VERSION {
            return Err(SigningError::InvalidKeyMaterial(format!(
                "unsupported plaintext dev signer key version {}",
                self.version
            )));
        }

        let secret_key = decode_hex32(&self.secret_key_hex, "secret_key_hex")?;
        let expected_public_key = decode_hex32(&self.public_key_hex, "public_key_hex")?;
        let keypair = AgentKeypair {
            secret: ed25519_dalek::SigningKey::from_bytes(&secret_key),
        };

        if keypair.identity().public_key != expected_public_key {
            return Err(SigningError::InvalidKeyMaterial(
                "public_key_hex does not match secret_key_hex".to_string(),
            ));
        }

        Ok(keypair)
    }

    /// Derive the public identity represented by this key material.
    pub fn identity(&self) -> Result<AgentIdentity, SigningError> {
        let mut identity = self.to_keypair()?.identity();
        identity.label = self.label.clone();
        Ok(identity)
    }
}

fn encode_hex32(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn decode_hex32(hex: &str, field: &str) -> Result<[u8; 32], SigningError> {
    let bytes = hex.as_bytes();
    if bytes.len() != 64 {
        return Err(SigningError::InvalidKeyMaterial(format!(
            "{field} must contain 64 hex characters"
        )));
    }

    let mut decoded = [0u8; 32];
    for (index, chunk) in bytes.chunks_exact(2).enumerate() {
        let high = decode_hex_nibble(chunk[0]).ok_or_else(|| {
            SigningError::InvalidKeyMaterial(format!("{field} contains a non-hex character"))
        })?;
        let low = decode_hex_nibble(chunk[1]).ok_or_else(|| {
            SigningError::InvalidKeyMaterial(format!("{field} contains a non-hex character"))
        })?;
        decoded[index] = (high << 4) | low;
    }

    Ok(decoded)
}

fn decode_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
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
