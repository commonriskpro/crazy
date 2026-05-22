// ── ail-stdlib::crypto ────────────────────────────────────────────────────
//
// Common cryptographic primitives for the AIL `std.crypto` module.
//
// # Rules (from docs/stdlib.md)
//
// - unsafe/custom crypto discouraged by policy
// - secrets not exposed as plain Text by default
// - constant-time comparisons explicit
//
// This module uses `blake3` (already a workspace dep) for hashing.
// HMAC is implemented with blake3 keyed hash.
// Password hashing and asymmetric crypto are type-level markers; actual
// implementations require host capabilities.

// ── SecureBytes ───────────────────────────────────────────────────────────

/// Opaque byte container for secrets. Does not implement `Display` or `Debug`
/// in a way that leaks values.
#[derive(Clone, PartialEq, Eq)]
pub struct SecureBytes(Vec<u8>);

impl SecureBytes {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for SecureBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SecureBytes([REDACTED; {} bytes])", self.0.len())
    }
}

// Intentionally NOT implementing Display to prevent accidental logging.

// ── Hash ──────────────────────────────────────────────────────────────────

/// A cryptographic hash output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hash(pub [u8; 32]);

impl Hash {
    /// Compute a BLAKE3 hash of the input bytes.
    pub fn blake3(data: &[u8]) -> Self {
        Self(*blake3::hash(data).as_bytes())
    }

    /// Return the hash bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Return the hash as a lowercase hex string.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

// ── ConstantTimeEq ────────────────────────────────────────────────────────

/// Constant-time equality comparison for byte slices.
///
/// Prevents timing side-channels. Per `docs/stdlib.md`: constant-time
/// comparisons must be explicit.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

// ── Hmac ──────────────────────────────────────────────────────────────────

/// An HMAC output computed using BLAKE3 keyed hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hmac(pub [u8; 32]);

impl Hmac {
    /// Compute HMAC-BLAKE3(key, data).
    ///
    /// Key must be exactly 32 bytes; if shorter/longer it is hashed first.
    pub fn compute(key: &[u8], data: &[u8]) -> Self {
        let key_hash = blake3::hash(key);
        let key_bytes: [u8; 32] = *key_hash.as_bytes();
        let mac = blake3::keyed_hash(&key_bytes, data);
        Self(*mac.as_bytes())
    }

    /// Verify an HMAC in constant time.
    pub fn verify(&self, key: &[u8], data: &[u8]) -> bool {
        let expected = Hmac::compute(key, data);
        constant_time_eq(&self.0, &expected.0)
    }

    /// Return as lowercase hex.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

// ── PasswordHash / Signature markers ─────────────────────────────────────

/// Opaque password hash (algorithm selected by runtime/policy).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasswordHash(pub String);

/// Opaque digital signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature(pub Vec<u8>);
