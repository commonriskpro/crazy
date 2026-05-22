// Tests for text.length_graphemes, time pure ops, and capability host
// extensions (clock.monotonic, random.bytes/generate).
//
// TDD: written BEFORE T10-T12 implementations.
// Spec: STDLIB-EXEC-TEXT-1..3, STDLIB-EXEC-TIME-1..3,
//       STDLIB-CAP-MONO-1..2, STDLIB-CAP-RAND-1

use ail_stdlib::exec::{InMemoryCapabilityHost, StdlibCapabilityDispatch, StdlibValue, call_pure_stdlib};

// ── STDLIB-EXEC-TEXT-1: length_graphemes ASCII ────────────────────────────

#[test]
fn text_length_graphemes_ascii() {
    let result = call_pure_stdlib(
        "std.text.length_graphemes",
        &[StdlibValue::Text("hello".to_string())],
    );
    assert_eq!(result, Ok(StdlibValue::Int(5)));
}

// ── STDLIB-EXEC-TEXT-2: length_graphemes multi-byte 1 grapheme ───────────

#[test]
fn text_length_graphemes_multibyte_single_grapheme() {
    // "é" is 2 bytes (UTF-8), 1 grapheme cluster
    let result = call_pure_stdlib(
        "std.text.length_graphemes",
        &[StdlibValue::Text("é".to_string())],
    );
    assert_eq!(result, Ok(StdlibValue::Int(1)));
}

// ── STDLIB-EXEC-TEXT-3: length_graphemes empty string ────────────────────

#[test]
fn text_length_graphemes_empty_string() {
    let result = call_pure_stdlib(
        "std.text.length_graphemes",
        &[StdlibValue::Text("".to_string())],
    );
    assert_eq!(result, Ok(StdlibValue::Int(0)));
}

// ── STDLIB-EXEC-TIME-1: duration_since returns delta in ms ───────────────

#[test]
fn time_duration_since_returns_delta() {
    let result = call_pure_stdlib(
        "std.time.duration_since",
        &[StdlibValue::Int(5000), StdlibValue::Int(3000)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(2000)));
}

// Triangulate: duration_since with larger delta
#[test]
fn time_duration_since_larger_delta() {
    let result = call_pure_stdlib(
        "std.time.duration_since",
        &[StdlibValue::Int(10_000), StdlibValue::Int(1_000)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(9_000)));
}

// ── STDLIB-EXEC-TIME-2: add_duration adds delta to instant ───────────────

#[test]
fn time_add_duration_returns_sum() {
    let result = call_pure_stdlib(
        "std.time.add_duration",
        &[StdlibValue::Int(1000), StdlibValue::Int(500)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(1500)));
}

// Triangulate: add_duration with zero delta
#[test]
fn time_add_duration_zero_delta_is_identity() {
    let result = call_pure_stdlib(
        "std.time.add_duration",
        &[StdlibValue::Int(42_000), StdlibValue::Int(0)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(42_000)));
}

// ── STDLIB-EXEC-TIME-3: instant_to_ms is identity ────────────────────────

#[test]
fn time_instant_to_ms_is_identity() {
    let result = call_pure_stdlib(
        "std.time.instant_to_ms",
        &[StdlibValue::Int(99_000)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(99_000)));
}

// Triangulate: instant_to_ms with zero
#[test]
fn time_instant_to_ms_zero() {
    let result = call_pure_stdlib(
        "std.time.instant_to_ms",
        &[StdlibValue::Int(0)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(0)));
}

// ── STDLIB-CAP-MONO-1: clock.monotonic defaults to 0 ─────────────────────

#[test]
fn clock_monotonic_defaults_to_zero() {
    let host = InMemoryCapabilityHost::new();
    let result = host.call("clock.monotonic", "now", &[]);
    assert_eq!(result, Ok(StdlibValue::Int(0)));
}

// ── STDLIB-CAP-MONO-2: clock.monotonic returns fixed value ───────────────

#[test]
fn clock_monotonic_returns_fixed_value() {
    let host = InMemoryCapabilityHost::new().with_monotonic(42_000);
    let result = host.call("clock.monotonic", "now", &[]);
    assert_eq!(result, Ok(StdlibValue::Int(42_000)));
}

// ── STDLIB-CAP-RAND-1: random.bytes returns Bytes of requested length ─────

#[test]
fn random_bytes_generate_returns_correct_length() {
    let host = InMemoryCapabilityHost::new();
    let result = host.call("random.bytes", "generate", &[StdlibValue::Int(4)]);
    assert!(
        matches!(result, Ok(StdlibValue::Bytes(ref b)) if b.len() == 4),
        "random.bytes/generate must return Bytes of length 4, got: {:?}",
        result
    );
}

// Triangulate: different length
#[test]
fn random_bytes_generate_length_16() {
    let host = InMemoryCapabilityHost::new();
    let result = host.call("random.bytes", "generate", &[StdlibValue::Int(16)]);
    assert!(
        matches!(result, Ok(StdlibValue::Bytes(ref b)) if b.len() == 16),
        "random.bytes/generate must return Bytes of length 16"
    );
}
