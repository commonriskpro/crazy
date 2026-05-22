// ── ail-package::signing ──────────────────────────────────────────────────
//
// Ed25519 package signing and verification.
//
// # Design decisions
//
// - Signing payload = UTF-8 bytes of `manifest.blake3_hex()` — the manifest's
//   own content hash.  This binds the signature to the full manifest content
//   without re-serialising the manifest inside the signing module.
// - `PackageSignature` stores raw bytes for CBOR determinism.  The 64-byte
//   signature uses a custom `sig_serde` shim (same pattern as `ail-remote`)
//   because `ciborium` does not support fixed-length arrays > 32 natively.
// - `PackageKeypair` wraps `ed25519_dalek::SigningKey` and never exposes the
//   secret key.
//
// # Dependency isolation
//
// This module does NOT depend on `ail-remote`.  The `sig_serde` shim is
// duplicated intentionally — keeping `ail-package` free of upward deps.

use ed25519_dalek::{Signer, Verifier};
use serde::{Deserialize, Serialize};

use crate::manifest::{PackageError, PackageManifest};

// ── sig_serde ─────────────────────────────────────────────────────────────
//
// Custom (de)serialization for `[u8; 64]` via CBOR byte string.

mod sig_serde {
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

/// Error returned by package signing and verification operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SigningError {
    /// The signature does not match the package manifest for the given signer.
    SignatureInvalid,
    /// The manifest content hash could not be computed.
    HashError(String),
}

impl std::fmt::Display for SigningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SigningError::SignatureInvalid => write!(f, "Ed25519 package signature is invalid"),
            SigningError::HashError(msg) => write!(f, "manifest hash error: {msg}"),
        }
    }
}

impl std::error::Error for SigningError {}

impl From<PackageError> for SigningError {
    fn from(e: PackageError) -> Self {
        SigningError::HashError(e.0)
    }
}

// ── PackageSignature ───────────────────────────────────────────────────────

/// An Ed25519 signature over a `PackageManifest` content hash.
///
/// `signer` is the raw 32-byte Ed25519 public key of the entity that signed
/// the manifest.  `signature` is the 64-byte raw Ed25519 signature over
/// `BLAKE3(CBOR(manifest))` expressed as UTF-8 hex bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageSignature {
    /// Raw 32-byte Ed25519 public key of the signer.
    pub signer: [u8; 32],
    /// Raw 64-byte Ed25519 signature.
    #[serde(with = "sig_serde")]
    pub signature: [u8; 64],
}

// ── SignedPackage ─────────────────────────────────────────────────────────

/// A `PackageManifest` with an attached Ed25519 signature.
///
/// Use [`SignedPackage::verify`] to confirm the signature is valid before
/// trusting the manifest content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedPackage {
    /// The signed package manifest.
    pub manifest: PackageManifest,
    /// The Ed25519 signature over the manifest content hash.
    pub sig: PackageSignature,
}

impl SignedPackage {
    /// Verify that the attached signature is valid for the embedded manifest.
    ///
    /// The signing payload is the UTF-8 bytes of `manifest.blake3_hex()`.
    ///
    /// # Errors
    ///
    /// - `SigningError::HashError` if `blake3_hex()` fails.
    /// - `SigningError::SignatureInvalid` if the key bytes are malformed or
    ///   the signature does not match.
    pub fn verify(&self) -> Result<(), SigningError> {
        let hash_hex = self.manifest.blake3_hex()?;
        let payload = hash_hex.as_bytes();

        let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&self.sig.signer)
            .map_err(|_| SigningError::SignatureInvalid)?;
        let signature = ed25519_dalek::Signature::from_bytes(&self.sig.signature);

        verifying_key
            .verify(payload, &signature)
            .map_err(|_| SigningError::SignatureInvalid)
    }
}

// ── PackageKeypair ────────────────────────────────────────────────────────

/// An Ed25519 signing keypair used to sign `PackageManifest` values.
///
/// The secret key is never exposed via public API.
pub struct PackageKeypair {
    secret: ed25519_dalek::SigningKey,
}

impl PackageKeypair {
    /// Construct a `PackageKeypair` from raw 32-byte secret key bytes.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the bytes do not represent a valid Ed25519 scalar
    /// (all 32-byte inputs are clamped by dalek so this never actually fails,
    /// but the API is kept explicit for forward compatibility).
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self {
            secret: ed25519_dalek::SigningKey::from_bytes(bytes),
        }
    }

    /// Derive the 32-byte public key for this keypair.
    pub fn public_key(&self) -> [u8; 32] {
        self.secret.verifying_key().to_bytes()
    }

    /// Sign a `PackageManifest` and return a `SignedPackage`.
    ///
    /// The signing payload is the UTF-8 bytes of `manifest.blake3_hex()`.
    ///
    /// # Errors
    ///
    /// Returns `SigningError::HashError` if `blake3_hex()` fails.
    pub fn sign_manifest(&self, manifest: PackageManifest) -> Result<SignedPackage, SigningError> {
        let hash_hex = manifest.blake3_hex()?;
        let payload = hash_hex.as_bytes();
        let signature: [u8; 64] = self.secret.sign(payload).to_bytes();
        let signer = self.public_key();
        Ok(SignedPackage {
            manifest,
            sig: PackageSignature { signer, signature },
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{PackageDef, PackageManifest};
    use crate::trust::TrustLevel;
    use rand::rngs::OsRng;

    fn minimal_manifest() -> PackageManifest {
        PackageManifest::from_def(PackageDef {
            name: "test.pkg".to_string(),
            version: "1.0.0".to_string(),
            trust_level: TrustLevel::Verified,
            required_capabilities: vec![],
            exported_capabilities: vec![],
            assumptions: vec![],
            unsafe_surface: vec![],
            artifact_hashes: vec![],
            build_env_hash: None,
            handlers: vec![],
            contracts: vec![],
            exports: vec![],
            imports: vec![],
            boundaries: vec![],
            license: None,
            provenance: None,
            verification_report: None,
            graph_schema: None,
            core_ir_schema: None,
        })
    }

    fn generate_keypair() -> PackageKeypair {
        let secret = ed25519_dalek::SigningKey::generate(&mut OsRng);
        PackageKeypair { secret }
    }

    // ── RED: package_signature_cbor_round_trip ────────────────────────────
    // Spec: REQ-SIGN-5 — SignedPackage is CBOR-serializable/deserializable
    //   GIVEN a SignedPackage with all fields set
    //   WHEN serialized to CBOR and deserialized
    //   THEN all fields are equal to the original
    #[test]
    fn signed_package_cbor_round_trip() {
        let kp = generate_keypair();
        let manifest = minimal_manifest();
        let signed = kp.sign_manifest(manifest).expect("sign must succeed");

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&signed, &mut buf).expect("CBOR serialize must succeed");
        let decoded: SignedPackage =
            ciborium::de::from_reader(buf.as_slice()).expect("CBOR deserialize must succeed");

        assert_eq!(decoded, signed, "round-tripped SignedPackage must equal original");
    }

    // ── RED: sign_verify_roundtrip ────────────────────────────────────────
    // Spec: REQ-SIGN-3, REQ-SIGN-4
    //   GIVEN a PackageKeypair and a PackageManifest
    //   WHEN sign_manifest is called and then verify is called on the result
    //   THEN verify returns Ok(())
    #[test]
    fn sign_verify_roundtrip() {
        let kp = generate_keypair();
        let manifest = minimal_manifest();
        let signed = kp.sign_manifest(manifest).expect("sign must succeed");
        signed.verify().expect("valid signature must verify successfully");
    }

    // ── RED: wrong_key_rejects_signature ─────────────────────────────────
    // Spec: REQ-SIGN-4 — wrong signer key returns SignatureInvalid
    //   GIVEN a SignedPackage signed by keypair A
    //   WHEN the signer field is replaced with keypair B's public key
    //   THEN verify returns Err(SigningError::SignatureInvalid)
    #[test]
    fn wrong_key_rejects_signature() {
        let kp_a = generate_keypair();
        let kp_b = generate_keypair();
        let manifest = minimal_manifest();
        let mut signed = kp_a.sign_manifest(manifest).expect("sign must succeed");
        // Tamper: replace signer with a different public key
        signed.sig.signer = kp_b.public_key();
        assert_eq!(
            signed.verify(),
            Err(SigningError::SignatureInvalid),
            "wrong signer key must return SignatureInvalid"
        );
    }

    // ── RED: tampered_manifest_rejects_signature ──────────────────────────
    // TRIANGULATE: if the manifest changes after signing, verify rejects it
    //   GIVEN a SignedPackage with an original manifest
    //   WHEN manifest.version is changed after signing
    //   THEN verify returns Err(SigningError::SignatureInvalid)
    #[test]
    fn tampered_manifest_rejects_signature() {
        let kp = generate_keypair();
        let manifest = minimal_manifest();
        let mut signed = kp.sign_manifest(manifest).expect("sign must succeed");
        // Tamper: modify manifest content
        signed.manifest.version = "9.9.9".to_string();
        assert_eq!(
            signed.verify(),
            Err(SigningError::SignatureInvalid),
            "tampered manifest must not verify"
        );
    }

    // ── RED: public_key_roundtrip ─────────────────────────────────────────
    // Spec: REQ-SIGN-1 — signer public key embedded in signature matches keypair
    //   GIVEN a PackageKeypair
    //   WHEN sign_manifest is called
    //   THEN signed.sig.signer == kp.public_key()
    #[test]
    fn public_key_embedded_in_signature() {
        let kp = generate_keypair();
        let manifest = minimal_manifest();
        let signed = kp.sign_manifest(manifest).expect("sign must succeed");
        assert_eq!(
            signed.sig.signer,
            kp.public_key(),
            "embedded signer must equal keypair public key"
        );
    }
}
