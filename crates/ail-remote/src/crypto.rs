// ── ail-remote::crypto ────────────────────────────────────────────────────
//
// Cryptographic primitives required by the decision log:
//
//   - AES-256-GCM  (symmetric authenticated encryption)
//   - Argon2id     (password-based key derivation)
//   - X25519       (Diffie-Hellman key exchange)
//
// This module is compiled only when the `crypto` feature is enabled.
// The workspace-level `deny(unsafe_code)` applies here; all three crates
// operate entirely in safe Rust from our side.
//
// # Error model
//
// Each primitive returns `CryptoError`.  Callers should treat all errors as
// opaque failures — no internal state is exposed through the error variants.

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit},
};
use argon2::{Algorithm, Argon2, Params, Version};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

// ── CryptoError ──────────────────────────────────────────────────────────

/// Error returned by crypto primitive operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CryptoError {
    /// AES-256-GCM encryption failed (should not happen for well-formed inputs).
    EncryptionFailed,
    /// AES-256-GCM decryption or authentication tag verification failed.
    DecryptionFailed,
    /// Argon2id key derivation failed.
    KeyDerivationFailed,
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CryptoError::EncryptionFailed => write!(f, "AES-256-GCM encryption failed"),
            CryptoError::DecryptionFailed => {
                write!(f, "AES-256-GCM decryption or tag verification failed")
            }
            CryptoError::KeyDerivationFailed => write!(f, "Argon2id key derivation failed"),
        }
    }
}

impl std::error::Error for CryptoError {}

// ── AES-256-GCM ──────────────────────────────────────────────────────────

/// Encrypt `plaintext` using AES-256-GCM.
///
/// # Parameters
///
/// - `key`   — 32-byte symmetric key.
/// - `nonce` — 12-byte nonce (GCM standard).  **MUST be unique per (key, message) pair.**
///   Reusing a nonce with the same key is a catastrophic security failure.
/// - `plaintext` — arbitrary byte slice to encrypt.
///
/// # Returns
///
/// The ciphertext with the 16-byte GCM authentication tag appended.
///
/// # Errors
///
/// Returns [`CryptoError::EncryptionFailed`] if the underlying AEAD cipher
/// reports an error (practically impossible for valid key/nonce lengths).
pub fn encrypt_aes256gcm(
    key: &[u8; 32],
    nonce: &[u8; 12],
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Nonce::from_slice(nonce);
    cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| CryptoError::EncryptionFailed)
}

/// Decrypt and authenticate `ciphertext` using AES-256-GCM.
///
/// # Parameters
///
/// - `key`        — 32-byte symmetric key (must match the key used to encrypt).
/// - `nonce`      — 12-byte nonce (must match the nonce used to encrypt).
/// - `ciphertext` — encrypted bytes with the 16-byte GCM tag appended.
///
/// # Returns
///
/// The decrypted plaintext on success.
///
/// # Errors
///
/// Returns [`CryptoError::DecryptionFailed`] if the authentication tag does
/// not match (tampered ciphertext, wrong key, or wrong nonce).
pub fn decrypt_aes256gcm(
    key: &[u8; 32],
    nonce: &[u8; 12],
    ciphertext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Nonce::from_slice(nonce);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| CryptoError::DecryptionFailed)
}

// ── Argon2id ─────────────────────────────────────────────────────────────

/// Derive a 32-byte key from `password` and `salt` using Argon2id.
///
/// Parameters are fixed to OWASP-recommended minimums for interactive use:
/// - Memory: 64 MiB (`m = 65536`)
/// - Iterations: 3 (`t = 3`)
/// - Parallelism: 1 (`p = 1`)
///
/// For offline or high-security contexts, callers should use a purpose-built
/// wrapper that increases memory and time cost.
///
/// # Parameters
///
/// - `password` — arbitrary byte slice (user password or secret material).
/// - `salt`     — 16-byte random salt.  **MUST be unique per credential.**
///
/// # Returns
///
/// A 32-byte derived key suitable for use as an AES-256 key or similar.
///
/// # Errors
///
/// Returns [`CryptoError::KeyDerivationFailed`] if Argon2id reports an
/// internal error (e.g., invalid parameter combination).
pub fn derive_key_argon2(password: &[u8], salt: &[u8; 16]) -> Result<[u8; 32], CryptoError> {
    // OWASP interactive minimum: m=65536, t=3, p=1.
    let params =
        Params::new(65536, 3, 1, Some(32)).map_err(|_| CryptoError::KeyDerivationFailed)?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut output = [0u8; 32];
    argon2
        .hash_password_into(password, salt, &mut output)
        .map_err(|_| CryptoError::KeyDerivationFailed)?;
    Ok(output)
}

// ── X25519 ───────────────────────────────────────────────────────────────

/// Compute an X25519 Diffie-Hellman shared secret.
///
/// Both parties compute `shared = DH(my_secret, their_public)`.  The result
/// is the raw 32-byte Montgomery point; callers MUST pass it through a KDF
/// (e.g., [`derive_key_argon2`] or HKDF) before using it as a symmetric key.
///
/// # Parameters
///
/// - `my_secret`     — 32-byte scalar (our X25519 static secret).
/// - `their_public`  — 32-byte u-coordinate of the peer's public key.
///
/// # Returns
///
/// The 32-byte shared secret (raw DH output — KDF before use as a key).
pub fn x25519_shared_secret(my_secret: &[u8; 32], their_public: &[u8; 32]) -> [u8; 32] {
    let secret = StaticSecret::from(*my_secret);
    let public = X25519PublicKey::from(*their_public);
    secret.diffie_hellman(&public).to_bytes()
}

// ── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── aes256gcm_encrypt_decrypt_roundtrip ───────────────────────────────
    // Encrypt plaintext then decrypt; result must equal original.
    #[test]
    fn aes256gcm_encrypt_decrypt_roundtrip() {
        let key = [0x42u8; 32];
        let nonce = [0x01u8; 12];
        let plaintext = b"hello, AES-256-GCM";

        let ciphertext = encrypt_aes256gcm(&key, &nonce, plaintext).expect("encrypt must succeed");
        assert_ne!(
            ciphertext.as_slice(),
            plaintext.as_slice(),
            "ciphertext must differ from plaintext"
        );

        let recovered = decrypt_aes256gcm(&key, &nonce, &ciphertext).expect("decrypt must succeed");
        assert_eq!(
            recovered, plaintext,
            "decrypted output must match original plaintext"
        );
    }

    // ── aes256gcm_wrong_key_fails_decryption ──────────────────────────────
    // Decrypting with a different key must return DecryptionFailed.
    #[test]
    fn aes256gcm_wrong_key_fails_decryption() {
        let key = [0x42u8; 32];
        let wrong_key = [0xFFu8; 32];
        let nonce = [0x01u8; 12];
        let plaintext = b"authenticated payload";

        let ciphertext = encrypt_aes256gcm(&key, &nonce, plaintext).expect("encrypt must succeed");
        let result = decrypt_aes256gcm(&wrong_key, &nonce, &ciphertext);
        assert_eq!(
            result,
            Err(CryptoError::DecryptionFailed),
            "wrong key must cause DecryptionFailed"
        );
    }

    // ── aes256gcm_tampered_ciphertext_fails_auth ──────────────────────────
    // A single flipped bit in the ciphertext must fail authentication.
    #[test]
    fn aes256gcm_tampered_ciphertext_fails_auth() {
        let key = [0x11u8; 32];
        let nonce = [0x22u8; 12];
        let plaintext = b"tamper test payload";

        let mut ciphertext =
            encrypt_aes256gcm(&key, &nonce, plaintext).expect("encrypt must succeed");
        // Flip a bit in the first byte of the ciphertext body.
        ciphertext[0] ^= 0x01;
        let result = decrypt_aes256gcm(&key, &nonce, &ciphertext);
        assert_eq!(
            result,
            Err(CryptoError::DecryptionFailed),
            "tampered ciphertext must cause DecryptionFailed"
        );
    }

    // ── derive_key_argon2_deterministic ───────────────────────────────────
    // Same password + salt must produce the same key on repeated calls.
    #[test]
    fn derive_key_argon2_deterministic() {
        let password = b"correct horse battery staple";
        let salt = [0xABu8; 16];

        let key_a = derive_key_argon2(password, &salt).expect("derive must succeed");
        let key_b = derive_key_argon2(password, &salt).expect("derive must succeed");
        assert_eq!(
            key_a, key_b,
            "Argon2id must be deterministic for same inputs"
        );
    }

    // ── derive_key_argon2_different_salt_different_key ────────────────────
    // Different salt must produce a different key.
    #[test]
    fn derive_key_argon2_different_salt_different_key() {
        let password = b"correct horse battery staple";
        let salt_a = [0x01u8; 16];
        let salt_b = [0x02u8; 16];

        let key_a = derive_key_argon2(password, &salt_a).expect("derive must succeed");
        let key_b = derive_key_argon2(password, &salt_b).expect("derive must succeed");
        assert_ne!(key_a, key_b, "different salts must produce different keys");
    }

    // ── x25519_shared_secret_symmetric ────────────────────────────────────
    // Both sides must derive the same shared secret (DH symmetry).
    #[test]
    fn x25519_shared_secret_symmetric() {
        use rand::RngCore;
        let mut rng = rand::thread_rng();

        let mut alice_secret_bytes = [0u8; 32];
        let mut bob_secret_bytes = [0u8; 32];
        rng.fill_bytes(&mut alice_secret_bytes);
        rng.fill_bytes(&mut bob_secret_bytes);

        // Derive public keys via x25519_dalek directly (for test setup only).
        let alice_secret = x25519_dalek::StaticSecret::from(alice_secret_bytes);
        let bob_secret = x25519_dalek::StaticSecret::from(bob_secret_bytes);
        let alice_public: [u8; 32] = x25519_dalek::PublicKey::from(&alice_secret).to_bytes();
        let bob_public: [u8; 32] = x25519_dalek::PublicKey::from(&bob_secret).to_bytes();

        let alice_shared = x25519_shared_secret(&alice_secret_bytes, &bob_public);
        let bob_shared = x25519_shared_secret(&bob_secret_bytes, &alice_public);

        assert_eq!(
            alice_shared, bob_shared,
            "X25519 DH must be symmetric: Alice and Bob derive the same shared secret"
        );
    }

    // ── x25519_different_peers_different_secrets ──────────────────────────
    // A third party's secret must not produce the same shared secret.
    #[test]
    fn x25519_different_peers_different_secrets() {
        use rand::RngCore;
        let mut rng = rand::thread_rng();

        let mut alice_secret_bytes = [0u8; 32];
        let mut bob_secret_bytes = [0u8; 32];
        let mut eve_secret_bytes = [0u8; 32];
        rng.fill_bytes(&mut alice_secret_bytes);
        rng.fill_bytes(&mut bob_secret_bytes);
        rng.fill_bytes(&mut eve_secret_bytes);

        let alice_secret = x25519_dalek::StaticSecret::from(alice_secret_bytes);
        let bob_secret = x25519_dalek::StaticSecret::from(bob_secret_bytes);
        let bob_public: [u8; 32] = x25519_dalek::PublicKey::from(&bob_secret).to_bytes();
        let alice_public: [u8; 32] = x25519_dalek::PublicKey::from(&alice_secret).to_bytes();

        let alice_bob_shared = x25519_shared_secret(&alice_secret_bytes, &bob_public);
        let eve_alice_shared = x25519_shared_secret(&eve_secret_bytes, &alice_public);

        assert_ne!(
            alice_bob_shared, eve_alice_shared,
            "Eve must not derive the same shared secret as Alice+Bob"
        );
    }
}
