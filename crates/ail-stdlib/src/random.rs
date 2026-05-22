// ── ail-stdlib::random ────────────────────────────────────────────────────
//
// Seeded random generation for the AIL `std.random` module.
//
// # Rules (from docs/stdlib.md)
//
// - randomness is not pure
// - deterministic and crypto randomness are separate
//
// Deterministic: LCG seeded by caller.
// Crypto: type-level marker only (actual crypto random bytes require
//   the `crypto.random.bytes` capability provided by the runtime).

// ── Seed ──────────────────────────────────────────────────────────────────

/// Opaque seed for deterministic RNG.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seed(pub u64);

impl Seed {
    pub fn new(v: u64) -> Self {
        Self(v)
    }
}

// ── DeterministicRng ──────────────────────────────────────────────────────

/// A simple Linear Congruential Generator (LCG) seeded deterministically.
///
/// Not suitable for security-sensitive use — use `CryptoRng` marker instead.
#[derive(Clone, Debug)]
pub struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    /// Create a new RNG from the given seed.
    pub fn new(seed: Seed) -> Self {
        Self { state: seed.0 }
    }

    /// Advance the state and return the next `u64`.
    fn next_u64(&mut self) -> u64 {
        // Knuth multiplicative LCG (mod 2^64)
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    /// Generate a random `i64` value.
    pub fn random_int(&mut self) -> i64 {
        self.next_u64() as i64
    }

    /// Generate a random `f64` in `[0.0, 1.0)`.
    pub fn random_float(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Generate `n` random bytes.
    pub fn random_bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| (self.next_u64() & 0xFF) as u8).collect()
    }

    /// Generate a random `i64` in the range `[min, max)`.
    pub fn random_int_range(&mut self, min: i64, max: i64) -> Option<i64> {
        if min >= max {
            return None;
        }
        let range = (max - min) as u64;
        Some(min + (self.next_u64() % range) as i64)
    }
}

// ── CryptoRng ─────────────────────────────────────────────────────────────

/// Marker for cryptographically-secure randomness.
///
/// Actual crypto-random byte generation requires the `crypto.random.bytes`
/// capability provided by the AIL runtime host. This type is the API surface;
/// the runtime binds the implementation.
#[derive(Debug)]
pub struct CryptoRng;

impl CryptoRng {
    /// Marker constructor. Runtime must inject capability.
    pub fn new() -> Self {
        Self
    }
}

impl Default for CryptoRng {
    fn default() -> Self {
        Self::new()
    }
}

// ── RandomBytes ───────────────────────────────────────────────────────────

/// A fixed-size array of random bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RandomBytes(pub Vec<u8>);

impl RandomBytes {
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
