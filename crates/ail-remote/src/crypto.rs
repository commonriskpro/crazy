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
// Each primitive returns `CryptoError`.  Failures expose stable issue codes,
// categories, and redacted messages so production callers can branch or log
// safely without leaking secret key, nonce, salt, password, or ciphertext bytes.

use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit},
};
use argon2::{Algorithm, Argon2, Params, Version};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

// ── CryptoError ──────────────────────────────────────────────────────────

const AES256_GCM_KEY_LEN: usize = 32;
const AES256_GCM_NONCE_LEN: usize = 12;
const AES256_GCM_TAG_LEN: usize = 16;
const ARGON2_SALT_LEN: usize = 16;

/// Stable machine-readable crypto issue categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CryptoIssueCategory {
    AesGcmInput,
    AesGcmOperation,
    Argon2Input,
    Argon2Operation,
}

impl CryptoIssueCategory {
    /// Stable lowercase category string for logs and telemetry.
    pub const fn as_str(self) -> &'static str {
        match self {
            CryptoIssueCategory::AesGcmInput => "aes_gcm_input",
            CryptoIssueCategory::AesGcmOperation => "aes_gcm_operation",
            CryptoIssueCategory::Argon2Input => "argon2_input",
            CryptoIssueCategory::Argon2Operation => "argon2_operation",
        }
    }
}

/// Stable machine-readable crypto issue codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CryptoIssueCode {
    Aes256GcmInvalidKeyLength,
    Aes256GcmInvalidNonceLength,
    Aes256GcmInvalidCiphertextLength,
    Aes256GcmEncryptionFailed,
    Aes256GcmDecryptionFailed,
    Argon2EmptyPassword,
    Argon2InvalidSaltLength,
    Argon2KeyDerivationFailed,
}

impl CryptoIssueCode {
    /// Stable uppercase issue code for API errors, logs, and telemetry.
    pub const fn as_str(self) -> &'static str {
        match self {
            CryptoIssueCode::Aes256GcmInvalidKeyLength => {
                "REMOTE_CRYPTO_AES256_GCM_INVALID_KEY_LENGTH"
            }
            CryptoIssueCode::Aes256GcmInvalidNonceLength => {
                "REMOTE_CRYPTO_AES256_GCM_INVALID_NONCE_LENGTH"
            }
            CryptoIssueCode::Aes256GcmInvalidCiphertextLength => {
                "REMOTE_CRYPTO_AES256_GCM_INVALID_CIPHERTEXT_LENGTH"
            }
            CryptoIssueCode::Aes256GcmEncryptionFailed => {
                "REMOTE_CRYPTO_AES256_GCM_ENCRYPTION_FAILED"
            }
            CryptoIssueCode::Aes256GcmDecryptionFailed => {
                "REMOTE_CRYPTO_AES256_GCM_DECRYPTION_FAILED"
            }
            CryptoIssueCode::Argon2EmptyPassword => "REMOTE_CRYPTO_ARGON2_EMPTY_PASSWORD",
            CryptoIssueCode::Argon2InvalidSaltLength => "REMOTE_CRYPTO_ARGON2_INVALID_SALT_LENGTH",
            CryptoIssueCode::Argon2KeyDerivationFailed => {
                "REMOTE_CRYPTO_ARGON2_KEY_DERIVATION_FAILED"
            }
        }
    }
}

/// Redacted, deterministic descriptor for a crypto failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CryptoIssueDescriptor {
    pub code: CryptoIssueCode,
    pub category: CryptoIssueCategory,
    /// Redacted human-readable message. Never includes secret input bytes.
    pub message: &'static str,
}

/// Error returned by crypto primitive operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CryptoError {
    /// AES-256-GCM key length is invalid.
    InvalidAes256GcmKeyLength { expected: usize, actual: usize },
    /// AES-256-GCM nonce length is invalid.
    InvalidAes256GcmNonceLength { expected: usize, actual: usize },
    /// AES-256-GCM ciphertext is too short to contain an authentication tag.
    InvalidAes256GcmCiphertextLength { minimum: usize, actual: usize },
    /// Argon2id password is empty.
    InvalidArgon2Password,
    /// Argon2id salt length is invalid.
    InvalidArgon2SaltLength { expected: usize, actual: usize },
    /// AES-256-GCM encryption failed (should not happen for well-formed inputs).
    EncryptionFailed,
    /// AES-256-GCM decryption or authentication tag verification failed.
    DecryptionFailed,
    /// Argon2id key derivation failed.
    KeyDerivationFailed,
}

impl CryptoError {
    /// Stable issue code for machine-readable handling.
    pub const fn code(&self) -> CryptoIssueCode {
        self.descriptor().code
    }

    /// Stable issue category for grouping crypto failures.
    pub const fn category(&self) -> CryptoIssueCategory {
        self.descriptor().category
    }

    /// Redacted deterministic descriptor safe for production diagnostics.
    pub const fn descriptor(&self) -> CryptoIssueDescriptor {
        match self {
            CryptoError::InvalidAes256GcmKeyLength { .. } => CryptoIssueDescriptor {
                code: CryptoIssueCode::Aes256GcmInvalidKeyLength,
                category: CryptoIssueCategory::AesGcmInput,
                message: "AES-256-GCM key must be exactly 32 bytes",
            },
            CryptoError::InvalidAes256GcmNonceLength { .. } => CryptoIssueDescriptor {
                code: CryptoIssueCode::Aes256GcmInvalidNonceLength,
                category: CryptoIssueCategory::AesGcmInput,
                message: "AES-256-GCM nonce must be exactly 12 bytes",
            },
            CryptoError::InvalidAes256GcmCiphertextLength { .. } => CryptoIssueDescriptor {
                code: CryptoIssueCode::Aes256GcmInvalidCiphertextLength,
                category: CryptoIssueCategory::AesGcmInput,
                message: "AES-256-GCM ciphertext must include a 16-byte authentication tag",
            },
            CryptoError::InvalidArgon2Password => CryptoIssueDescriptor {
                code: CryptoIssueCode::Argon2EmptyPassword,
                category: CryptoIssueCategory::Argon2Input,
                message: "Argon2id password must not be empty",
            },
            CryptoError::InvalidArgon2SaltLength { .. } => CryptoIssueDescriptor {
                code: CryptoIssueCode::Argon2InvalidSaltLength,
                category: CryptoIssueCategory::Argon2Input,
                message: "Argon2id salt must be exactly 16 bytes",
            },
            CryptoError::EncryptionFailed => CryptoIssueDescriptor {
                code: CryptoIssueCode::Aes256GcmEncryptionFailed,
                category: CryptoIssueCategory::AesGcmOperation,
                message: "AES-256-GCM encryption failed",
            },
            CryptoError::DecryptionFailed => CryptoIssueDescriptor {
                code: CryptoIssueCode::Aes256GcmDecryptionFailed,
                category: CryptoIssueCategory::AesGcmOperation,
                message: "AES-256-GCM decryption or tag verification failed",
            },
            CryptoError::KeyDerivationFailed => CryptoIssueDescriptor {
                code: CryptoIssueCode::Argon2KeyDerivationFailed,
                category: CryptoIssueCategory::Argon2Operation,
                message: "Argon2id key derivation failed",
            },
        }
    }
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.descriptor().message)
    }
}

impl std::error::Error for CryptoError {}

// ── AES-256-GCM ──────────────────────────────────────────────────────────

/// Encrypt `plaintext` using AES-256-GCM with runtime input validation.
///
/// This is the production-facing variant for callers that receive keys or
/// nonces as byte slices.  Validation errors are machine-readable and never
/// include key, nonce, or plaintext bytes.
pub fn try_encrypt_aes256gcm(
    key: &[u8],
    nonce: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    if key.len() != AES256_GCM_KEY_LEN {
        return Err(CryptoError::InvalidAes256GcmKeyLength {
            expected: AES256_GCM_KEY_LEN,
            actual: key.len(),
        });
    }
    if nonce.len() != AES256_GCM_NONCE_LEN {
        return Err(CryptoError::InvalidAes256GcmNonceLength {
            expected: AES256_GCM_NONCE_LEN,
            actual: nonce.len(),
        });
    }

    let key = <&[u8; AES256_GCM_KEY_LEN]>::try_from(key).expect("validated AES-256-GCM key length");
    let nonce =
        <&[u8; AES256_GCM_NONCE_LEN]>::try_from(nonce).expect("validated AES-256-GCM nonce length");

    encrypt_aes256gcm(key, nonce, plaintext)
}

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

/// Decrypt and authenticate `ciphertext` using AES-256-GCM with runtime input validation.
///
/// This is the production-facing variant for callers that receive keys, nonces,
/// or ciphertext as byte slices. Validation failures are returned before AEAD
/// authentication and never include secret input bytes.
pub fn try_decrypt_aes256gcm(
    key: &[u8],
    nonce: &[u8],
    ciphertext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    if key.len() != AES256_GCM_KEY_LEN {
        return Err(CryptoError::InvalidAes256GcmKeyLength {
            expected: AES256_GCM_KEY_LEN,
            actual: key.len(),
        });
    }
    if nonce.len() != AES256_GCM_NONCE_LEN {
        return Err(CryptoError::InvalidAes256GcmNonceLength {
            expected: AES256_GCM_NONCE_LEN,
            actual: nonce.len(),
        });
    }
    if ciphertext.len() < AES256_GCM_TAG_LEN {
        return Err(CryptoError::InvalidAes256GcmCiphertextLength {
            minimum: AES256_GCM_TAG_LEN,
            actual: ciphertext.len(),
        });
    }

    let key = <&[u8; AES256_GCM_KEY_LEN]>::try_from(key).expect("validated AES-256-GCM key length");
    let nonce =
        <&[u8; AES256_GCM_NONCE_LEN]>::try_from(nonce).expect("validated AES-256-GCM nonce length");

    decrypt_aes256gcm(key, nonce, ciphertext)
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

/// Derive a 32-byte key from `password` and `salt` using Argon2id with runtime validation.
///
/// This production-facing variant rejects empty passwords and invalid salt
/// lengths before derivation. Diagnostics never include password or salt bytes.
pub fn try_derive_key_argon2(password: &[u8], salt: &[u8]) -> Result<[u8; 32], CryptoError> {
    if password.is_empty() {
        return Err(CryptoError::InvalidArgon2Password);
    }
    if salt.len() != ARGON2_SALT_LEN {
        return Err(CryptoError::InvalidArgon2SaltLength {
            expected: ARGON2_SALT_LEN,
            actual: salt.len(),
        });
    }

    let salt = <&[u8; ARGON2_SALT_LEN]>::try_from(salt).expect("validated Argon2id salt length");

    derive_key_argon2(password, salt)
}

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

    // ── crypto_error_descriptors_are_stable_and_redacted ─────────────────
    // Machine-readable diagnostics must be deterministic and avoid secret bytes.
    #[test]
    fn crypto_error_descriptors_are_stable_and_redacted() {
        let error = CryptoError::InvalidAes256GcmKeyLength {
            expected: 32,
            actual: 31,
        };

        let descriptor = error.descriptor();
        assert_eq!(
            descriptor.code.as_str(),
            "REMOTE_CRYPTO_AES256_GCM_INVALID_KEY_LENGTH"
        );
        assert_eq!(descriptor.category.as_str(), "aes_gcm_input");
        assert_eq!(
            descriptor.message,
            "AES-256-GCM key must be exactly 32 bytes"
        );
        assert_eq!(error.to_string(), descriptor.message);
        assert!(
            !error.to_string().contains("171"),
            "diagnostic must not leak key byte values"
        );
    }

    // ── try_aes256gcm_reports_structural_input_errors ────────────────────
    // Runtime-slice APIs must classify invalid key, nonce, and ciphertext shapes.
    #[test]
    fn try_aes256gcm_reports_structural_input_errors() {
        let key = [0xABu8; 32];
        let nonce = [0xCDu8; 12];

        let key_error = try_encrypt_aes256gcm(&key[..31], &nonce, b"payload")
            .expect_err("short key must be rejected before encryption");
        assert_eq!(
            key_error,
            CryptoError::InvalidAes256GcmKeyLength {
                expected: 32,
                actual: 31,
            }
        );
        assert_eq!(
            key_error.code().as_str(),
            "REMOTE_CRYPTO_AES256_GCM_INVALID_KEY_LENGTH"
        );

        let nonce_error = try_encrypt_aes256gcm(&key, &nonce[..11], b"payload")
            .expect_err("short nonce must be rejected before encryption");
        assert_eq!(
            nonce_error,
            CryptoError::InvalidAes256GcmNonceLength {
                expected: 12,
                actual: 11,
            }
        );
        assert_eq!(
            nonce_error.code().as_str(),
            "REMOTE_CRYPTO_AES256_GCM_INVALID_NONCE_LENGTH"
        );

        let ciphertext_error = try_decrypt_aes256gcm(&key, &nonce, &[0xEF; 15])
            .expect_err("ciphertext without a full tag must be rejected before decrypt");
        assert_eq!(
            ciphertext_error,
            CryptoError::InvalidAes256GcmCiphertextLength {
                minimum: 16,
                actual: 15,
            }
        );
        assert_eq!(
            ciphertext_error.code().as_str(),
            "REMOTE_CRYPTO_AES256_GCM_INVALID_CIPHERTEXT_LENGTH"
        );
    }

    // ── try_argon2_reports_structural_input_errors ───────────────────────
    // Runtime-slice APIs must classify invalid password and salt shapes.
    #[test]
    fn try_argon2_reports_structural_input_errors() {
        let salt = [0x11u8; 16];

        let password_error = try_derive_key_argon2(b"", &salt)
            .expect_err("empty password must be rejected before derivation");
        assert_eq!(password_error, CryptoError::InvalidArgon2Password);
        assert_eq!(
            password_error.code().as_str(),
            "REMOTE_CRYPTO_ARGON2_EMPTY_PASSWORD"
        );
        assert_eq!(password_error.category().as_str(), "argon2_input");

        let salt_error = try_derive_key_argon2(b"password", &salt[..15])
            .expect_err("short salt must be rejected before derivation");
        assert_eq!(
            salt_error,
            CryptoError::InvalidArgon2SaltLength {
                expected: 16,
                actual: 15,
            }
        );
        assert_eq!(
            salt_error.code().as_str(),
            "REMOTE_CRYPTO_ARGON2_INVALID_SALT_LENGTH"
        );
        assert!(
            !salt_error.to_string().contains("17"),
            "diagnostic must not leak salt byte values"
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
