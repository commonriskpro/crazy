// Tests for text.length_graphemes, time pure ops, text.regex semantics, and
// capability host extensions (clock.monotonic, random.bytes/generate).
//
// TDD: written BEFORE T10-T12 implementations.
// Spec: STDLIB-EXEC-TEXT-1..3, STDLIB-EXEC-TIME-1..3,
//       STDLIB-CAP-MONO-1..2, STDLIB-CAP-RAND-1,
//       STDLIB-EXEC-REGEX-1..3

use ail_stdlib::exec::{
    InMemoryCapabilityHost, StdlibCapabilityDispatch, StdlibExecError, StdlibValue,
    call_pure_stdlib,
};

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
    let result = call_pure_stdlib("std.time.instant_to_ms", &[StdlibValue::Int(99_000)]);
    assert_eq!(result, Ok(StdlibValue::Int(99_000)));
}

// Triangulate: instant_to_ms with zero
#[test]
fn time_instant_to_ms_zero() {
    let result = call_pure_stdlib("std.time.instant_to_ms", &[StdlibValue::Int(0)]);
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

// ── STDLIB-EXEC-REGEX-1: anchored pattern matches, not substring ──────────
//
// "^\d+$" is a valid regex that matches "12345".
// str::contains("^\d+$") on "12345" is false — proving real regex is used.

#[test]
fn text_regex_anchored_digit_pattern_matches() {
    let result = call_pure_stdlib(
        "std.text.regex",
        &[
            StdlibValue::Text("12345".to_string()),
            StdlibValue::Text(r"^\d+$".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Bool(true)));
}

// ── STDLIB-EXEC-REGEX-2: dot-star is regex quantifier, not literal ────────
//
// "foo.*bar" matches "foobazbar" via regex.
// str::contains("foo.*bar") on "foobazbar" is false — proving regex semantics.

#[test]
fn text_regex_dot_star_matches_where_substring_would_not() {
    let result = call_pure_stdlib(
        "std.text.regex",
        &[
            StdlibValue::Text("foobazbar".to_string()),
            StdlibValue::Text("foo.*bar".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Bool(true)));
}

// ── STDLIB-EXEC-REGEX-3: invalid pattern returns StdlibExecError::Message ─

#[test]
fn text_regex_invalid_pattern_yields_exec_error() {
    let result = call_pure_stdlib(
        "std.text.regex",
        &[
            StdlibValue::Text("anything".to_string()),
            StdlibValue::Text("[".to_string()),
        ],
    );
    assert!(
        matches!(result, Err(StdlibExecError::Message(ref msg)) if msg.starts_with("invalid regex")),
        "expected StdlibExecError::Message starting with 'invalid regex', got: {:?}",
        result
    );
}

// ── STDLIB-EXEC-NORM-1: normalize one-arg default is NFC ─────────────────

#[test]
fn text_normalize_one_arg_defaults_to_nfc() {
    // e + combining acute (NFD) should be recomposed to U+00E9 (NFC)
    let decomposed = "e\u{0301}";
    let result = call_pure_stdlib(
        "std.text.normalize",
        &[StdlibValue::Text(decomposed.to_string())],
    );
    assert_eq!(result, Ok(StdlibValue::Text("\u{00E9}".to_string())));
}

// ── STDLIB-EXEC-NORM-2: normalize two-arg explicit "nfc" ─────────────────

#[test]
fn text_normalize_two_arg_nfc_recomposes() {
    let decomposed = "e\u{0301}";
    let result = call_pure_stdlib(
        "std.text.normalize",
        &[
            StdlibValue::Text(decomposed.to_string()),
            StdlibValue::Text("nfc".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Text("\u{00E9}".to_string())));
}

// ── STDLIB-EXEC-NORM-3: normalize two-arg "nfd" decomposes ───────────────

#[test]
fn text_normalize_two_arg_nfd_decomposes() {
    let precomposed = "\u{00E9}";
    let result = call_pure_stdlib(
        "std.text.normalize",
        &[
            StdlibValue::Text(precomposed.to_string()),
            StdlibValue::Text("nfd".to_string()),
        ],
    );
    // NFD must produce 2 codepoints: e + combining acute
    match result {
        Ok(StdlibValue::Text(s)) => {
            let codepoints: Vec<char> = s.chars().collect();
            assert_eq!(codepoints.len(), 2, "NFD must decompose to 2 codepoints");
            assert_eq!(codepoints[0], 'e');
            assert_eq!(codepoints[1], '\u{0301}');
        }
        other => panic!("expected Ok(Text), got: {other:?}"),
    }
}

// ── STDLIB-EXEC-NORM-4: unknown form string yields Message error ──────────

#[test]
fn text_normalize_unknown_form_yields_message_error() {
    let result = call_pure_stdlib(
        "std.text.normalize",
        &[
            StdlibValue::Text("hello".to_string()),
            StdlibValue::Text("nfkc".to_string()),
        ],
    );
    assert!(
        matches!(result, Err(StdlibExecError::Message(ref msg)) if msg.contains("unknown normalization form")),
        "expected Message error for unknown form, got: {result:?}"
    );
}

// ── STDLIB-EXEC-PRED-1: starts_with returns true for matching prefix ──────

#[test]
fn text_starts_with_exec_matching_prefix() {
    let result = call_pure_stdlib(
        "std.text.starts_with",
        &[
            StdlibValue::Text("hello world".to_string()),
            StdlibValue::Text("hello".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Bool(true)));
}

// ── STDLIB-EXEC-PRED-2: starts_with returns false for non-matching prefix ─

#[test]
fn text_starts_with_exec_non_matching_prefix() {
    let result = call_pure_stdlib(
        "std.text.starts_with",
        &[
            StdlibValue::Text("hello world".to_string()),
            StdlibValue::Text("world".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Bool(false)));
}

// ── STDLIB-EXEC-PRED-3: starts_with with empty prefix is always true ──────

#[test]
fn text_starts_with_exec_empty_prefix() {
    let result = call_pure_stdlib(
        "std.text.starts_with",
        &[
            StdlibValue::Text("hello".to_string()),
            StdlibValue::Text("".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Bool(true)));
}

// ── STDLIB-EXEC-PRED-4: ends_with returns true for matching suffix ────────

#[test]
fn text_ends_with_exec_matching_suffix() {
    let result = call_pure_stdlib(
        "std.text.ends_with",
        &[
            StdlibValue::Text("hello world".to_string()),
            StdlibValue::Text("world".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Bool(true)));
}

// ── STDLIB-EXEC-PRED-5: ends_with returns false for non-matching suffix ───

#[test]
fn text_ends_with_exec_non_matching_suffix() {
    let result = call_pure_stdlib(
        "std.text.ends_with",
        &[
            StdlibValue::Text("hello world".to_string()),
            StdlibValue::Text("hello".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Bool(false)));
}

// ── STDLIB-EXEC-PRED-6: ends_with with empty suffix is always true ────────

#[test]
fn text_ends_with_exec_empty_suffix() {
    let result = call_pure_stdlib(
        "std.text.ends_with",
        &[
            StdlibValue::Text("hello".to_string()),
            StdlibValue::Text("".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Bool(true)));
}

// ── STDLIB-EXEC-PRED-7: contains returns true when needle is present ──────

#[test]
fn text_contains_exec_substring_present() {
    let result = call_pure_stdlib(
        "std.text.contains",
        &[
            StdlibValue::Text("hello world".to_string()),
            StdlibValue::Text("lo wo".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Bool(true)));
}

// ── STDLIB-EXEC-PRED-8: contains returns false when needle is absent ──────

#[test]
fn text_contains_exec_substring_absent() {
    let result = call_pure_stdlib(
        "std.text.contains",
        &[
            StdlibValue::Text("hello world".to_string()),
            StdlibValue::Text("xyz".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Bool(false)));
}

// ── STDLIB-EXEC-PRED-9: contains with empty needle is always true ─────────

#[test]
fn text_contains_exec_empty_needle() {
    let result = call_pure_stdlib(
        "std.text.contains",
        &[
            StdlibValue::Text("hello".to_string()),
            StdlibValue::Text("".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Bool(true)));
}

// ── STDLIB-EXEC-PRED-10: replace substitutes all occurrences ─────────────

#[test]
fn text_replace_exec_replaces_all() {
    let result = call_pure_stdlib(
        "std.text.replace",
        &[
            StdlibValue::Text("aabbaa".to_string()),
            StdlibValue::Text("aa".to_string()),
            StdlibValue::Text("X".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Text("XbbX".to_string())));
}

// ── STDLIB-EXEC-PRED-11: replace with no match returns original ───────────

#[test]
fn text_replace_exec_no_match_returns_original() {
    let result = call_pure_stdlib(
        "std.text.replace",
        &[
            StdlibValue::Text("hello".to_string()),
            StdlibValue::Text("xyz".to_string()),
            StdlibValue::Text("Y".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Text("hello".to_string())));
}

// ── STDLIB-EXEC-PRED-12: replace with empty from returns original ─────────

#[test]
fn text_replace_exec_empty_from_returns_unchanged() {
    let result = call_pure_stdlib(
        "std.text.replace",
        &[
            StdlibValue::Text("hello".to_string()),
            StdlibValue::Text("".to_string()),
            StdlibValue::Text("X".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Text("hello".to_string())));
}

// ── STDLIB-EXEC-TEXT-INDEX: index_of mirrors source/compiler helper ──────

#[test]
fn text_index_of_exec_returns_first_byte_offset() {
    let result = call_pure_stdlib(
        "std.text.index_of",
        &[
            StdlibValue::Text("Hello, AIL".to_string()),
            StdlibValue::Text("AIL".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Int(7)));
}

#[test]
fn text_index_of_exec_returns_minus_one_when_absent() {
    let result = call_pure_stdlib(
        "std.text.index_of",
        &[
            StdlibValue::Text("hello".to_string()),
            StdlibValue::Text("xyz".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Int(-1)));
}

#[test]
fn text_index_of_exec_uses_utf8_byte_offsets() {
    let result = call_pure_stdlib(
        "std.text.index_of",
        &[
            StdlibValue::Text("🔥AIL".to_string()),
            StdlibValue::Text("AIL".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Int(4)));
}

// ── STDLIB-EXEC-TEXT-PARSE: parse_int_or mirrors source/compiler helper ──

#[test]
fn text_parse_int_or_exec_parses_signed_int() {
    let result = call_pure_stdlib(
        "std.text.parse_int_or",
        &[StdlibValue::Text("-42".to_string()), StdlibValue::Int(0)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(-42)));
}

#[test]
fn text_parse_int_or_exec_returns_fallback_on_invalid_syntax() {
    let result = call_pure_stdlib(
        "std.text.parse_int_or",
        &[StdlibValue::Text("42px".to_string()), StdlibValue::Int(-1)],
    );
    assert_eq!(result, Ok(StdlibValue::Int(-1)));
}

#[test]
fn text_parse_int_or_exec_returns_fallback_on_overflow() {
    let result = call_pure_stdlib(
        "std.text.parse_int_or",
        &[
            StdlibValue::Text("9223372036854775808".to_string()),
            StdlibValue::Int(-1),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Int(-1)));
}

#[test]
fn text_parse_int_or_entry_is_registered() {
    let entry = ail_stdlib::exec::find_function_entry("std.text.parse_int_or")
        .expect("std.text.parse_int_or entry");
    assert_eq!(entry.module, "std.text");
    assert_eq!(entry.params, ["Text", "Int"]);
    assert_eq!(entry.return_type, "Int");
}

// ── STDLIB-EXEC-TEXT-BYTE: byte_at_or mirrors source/compiler helper ─────

#[test]
fn text_byte_at_or_exec_returns_byte_value() {
    let result = call_pure_stdlib(
        "std.text.byte_at_or",
        &[
            StdlibValue::Text("AIL".to_string()),
            StdlibValue::Int(1),
            StdlibValue::Int(-1),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Int(73)));
}

#[test]
fn text_byte_at_or_exec_returns_fallback_when_out_of_range() {
    let result = call_pure_stdlib(
        "std.text.byte_at_or",
        &[
            StdlibValue::Text("AIL".to_string()),
            StdlibValue::Int(3),
            StdlibValue::Int(99),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Int(99)));
}

// ── STDLIB-EXEC-TEXT-SLICE: slice mirrors source/compiler helper ─────────

#[test]
fn text_slice_exec_returns_utf8_byte_slice() {
    let result = call_pure_stdlib(
        "std.text.slice",
        &[
            StdlibValue::Text("Hello, AIL".to_string()),
            StdlibValue::Int(7),
            StdlibValue::Int(3),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Text("AIL".to_string())));
}

#[test]
fn text_slice_exec_rejects_utf8_boundary_splits() {
    let result = call_pure_stdlib(
        "std.text.slice",
        &[
            StdlibValue::Text("éx".to_string()),
            StdlibValue::Int(1),
            StdlibValue::Int(1),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Text(String::new())));
}

// ── STDLIB-EXEC-TEXT-REPLACE-FIRST: mirrors source/compiler helper ───────

#[test]
fn text_replace_first_exec_replaces_only_first_occurrence() {
    let result = call_pure_stdlib(
        "std.text.replace_first",
        &[
            StdlibValue::Text("one one one".to_string()),
            StdlibValue::Text("one".to_string()),
            StdlibValue::Text("two".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Text("two one one".to_string())));
}

#[test]
fn text_replace_first_exec_empty_needle_returns_original() {
    let result = call_pure_stdlib(
        "std.text.replace_first",
        &[
            StdlibValue::Text("hello".to_string()),
            StdlibValue::Text("".to_string()),
            StdlibValue::Text("x".to_string()),
        ],
    );
    assert_eq!(result, Ok(StdlibValue::Text("hello".to_string())));
}

#[test]
fn text_slice_entry_is_registered() {
    let entry =
        ail_stdlib::exec::find_function_entry("std.text.slice").expect("std.text.slice entry");
    assert_eq!(entry.module, "std.text");
    assert_eq!(entry.params, ["Text", "Int", "Int"]);
    assert_eq!(entry.return_type, "Text");
}
