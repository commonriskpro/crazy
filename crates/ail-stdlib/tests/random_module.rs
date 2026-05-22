use ail_stdlib::random::{CryptoRng, DeterministicRng, RandomBytes, Seed};

#[test]
fn deterministic_rng_seeded_reproducible() {
    let seed = Seed::new(12345);
    let mut rng1 = DeterministicRng::new(seed);
    let mut rng2 = DeterministicRng::new(seed);
    // Same seed -> same sequence
    assert_eq!(rng1.random_int(), rng2.random_int());
    assert_eq!(rng1.random_int(), rng2.random_int());
}

#[test]
fn deterministic_rng_different_seeds_differ() {
    let mut rng1 = DeterministicRng::new(Seed::new(1));
    let mut rng2 = DeterministicRng::new(Seed::new(2));
    // Different seeds should produce different values (not guaranteed but true for LCG)
    let v1 = rng1.random_int();
    let v2 = rng2.random_int();
    assert_ne!(v1, v2);
}

#[test]
fn deterministic_rng_float_in_range() {
    let mut rng = DeterministicRng::new(Seed::new(42));
    for _ in 0..100 {
        let f = rng.random_float();
        assert!(f >= 0.0 && f < 1.0, "float {f} out of [0,1)");
    }
}

#[test]
fn deterministic_rng_bytes_length() {
    let mut rng = DeterministicRng::new(Seed::new(99));
    let bytes = rng.random_bytes(16);
    assert_eq!(bytes.len(), 16);
}

#[test]
fn deterministic_rng_int_range() {
    let mut rng = DeterministicRng::new(Seed::new(7));
    for _ in 0..100 {
        let v = rng.random_int_range(0, 10).unwrap();
        assert!(v >= 0 && v < 10, "value {v} out of [0,10)");
    }
}

#[test]
fn deterministic_rng_int_range_invalid() {
    let mut rng = DeterministicRng::new(Seed::new(7));
    assert_eq!(rng.random_int_range(10, 5), None);
}

#[test]
fn crypto_rng_marker() {
    let _ = CryptoRng::new(); // just ensure it constructs
}

#[test]
fn random_bytes_struct() {
    let rb = RandomBytes::new(vec![1, 2, 3]);
    assert_eq!(rb.len(), 3);
    assert_eq!(rb.as_slice(), &[1, 2, 3]);
    assert!(!rb.is_empty());
}
