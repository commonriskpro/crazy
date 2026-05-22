use ail_stdlib::encoding::{base64_decode, base64_encode, hex_decode, hex_encode};

#[test]
fn base64_encode_basic() {
    assert_eq!(base64_encode(b"Man"), "TWFu");
    assert_eq!(base64_encode(b""), "");
}

#[test]
fn base64_encode_with_padding() {
    // "Ma" -> "TWE="
    assert_eq!(base64_encode(b"Ma"), "TWE=");
    // "M" -> "TQ=="
    assert_eq!(base64_encode(b"M"), "TQ==");
}

#[test]
fn base64_roundtrip() {
    let data = b"Hello, World! This is a test.";
    let encoded = base64_encode(data);
    let decoded = base64_decode(&encoded).unwrap();
    assert_eq!(decoded, data);
}

#[test]
fn base64_decode_invalid_length() {
    assert!(base64_decode("abc").is_err()); // not multiple of 4
}

#[test]
fn hex_encode_basic() {
    assert_eq!(hex_encode(&[0x00, 0xFF, 0xAB]), "00ffab");
    assert_eq!(hex_encode(&[]), "");
}

#[test]
fn hex_decode_basic() {
    assert_eq!(hex_decode("00ffab").unwrap(), vec![0x00, 0xFF, 0xAB]);
}

#[test]
fn hex_decode_case_insensitive() {
    assert_eq!(
        hex_decode("DEADBEEF").unwrap(),
        hex_decode("deadbeef").unwrap()
    );
}

#[test]
fn hex_decode_odd_length_error() {
    assert!(hex_decode("abc").is_err());
}

#[test]
fn hex_roundtrip() {
    let data: Vec<u8> = (0u8..32).collect();
    let encoded = hex_encode(&data);
    let decoded = hex_decode(&encoded).unwrap();
    assert_eq!(decoded, data);
}
