use ail_stdlib::crypto::{Hash, Hmac, PasswordHash, SecureBytes, Signature, constant_time_eq};

#[test]
fn hash_blake3_deterministic() {
    let h1 = Hash::blake3(b"hello world");
    let h2 = Hash::blake3(b"hello world");
    assert_eq!(h1, h2);
}

#[test]
fn hash_different_inputs_differ() {
    let h1 = Hash::blake3(b"foo");
    let h2 = Hash::blake3(b"bar");
    assert_ne!(h1, h2);
}

#[test]
fn hash_to_hex_length() {
    let h = Hash::blake3(b"test");
    assert_eq!(h.to_hex().len(), 64); // 32 bytes = 64 hex chars
}

#[test]
fn hash_to_hex_lowercase() {
    let h = Hash::blake3(b"test");
    assert!(
        h.to_hex()
            .chars()
            .all(|c| c.is_ascii_alphanumeric() && !c.is_ascii_uppercase())
    );
}

#[test]
fn constant_time_eq_equal() {
    assert!(constant_time_eq(b"hello", b"hello"));
}

#[test]
fn constant_time_eq_not_equal() {
    assert!(!constant_time_eq(b"hello", b"world"));
}

#[test]
fn constant_time_eq_different_lengths() {
    assert!(!constant_time_eq(b"hello", b"hi"));
}

#[test]
fn hmac_compute_deterministic() {
    let h1 = Hmac::compute(b"key", b"message");
    let h2 = Hmac::compute(b"key", b"message");
    assert_eq!(h1, h2);
}

#[test]
fn hmac_different_keys_differ() {
    let h1 = Hmac::compute(b"key1", b"message");
    let h2 = Hmac::compute(b"key2", b"message");
    assert_ne!(h1, h2);
}

#[test]
fn hmac_verify_correct() {
    let key = b"secret_key";
    let data = b"important data";
    let mac = Hmac::compute(key, data);
    assert!(mac.verify(key, data));
}

#[test]
fn hmac_verify_tampered_data() {
    let key = b"secret_key";
    let mac = Hmac::compute(key, b"original");
    assert!(!mac.verify(key, b"tampered"));
}

#[test]
fn hmac_to_hex_length() {
    let h = Hmac::compute(b"k", b"d");
    assert_eq!(h.to_hex().len(), 64);
}

#[test]
fn secure_bytes_debug_redacted() {
    let sb = SecureBytes::new(vec![1, 2, 3]);
    let debug_str = format!("{:?}", sb);
    assert!(debug_str.contains("REDACTED"));
    assert!(!debug_str.contains("1, 2, 3"));
}

#[test]
fn secure_bytes_len() {
    let sb = SecureBytes::new(vec![0u8; 16]);
    assert_eq!(sb.len(), 16);
    assert!(!sb.is_empty());
}

#[test]
fn password_hash_wrapper() {
    let ph = PasswordHash("$argon2id$...".into());
    assert!(ph.0.starts_with('$'));
}

#[test]
fn signature_wrapper() {
    let sig = Signature(vec![0u8; 64]);
    assert_eq!(sig.0.len(), 64);
}
