// Tests for ail-stdlib::text — text helpers.
//
// TDD cycle: tests written before implementation.
// Spec: G26 stdlib-impl, Requirements R4.1–R4.6.

use ail_stdlib::text::{
    NormalizeForm, text_contains, text_ends_with, text_from_bytes, text_join,
    text_length_graphemes, text_normalize, text_replace, text_split, text_starts_with,
    text_to_bytes, text_trim,
};

// ── R4.1: text_trim ──────────────────────────────────────────────────────

// S4.1: removes leading and trailing whitespace
#[test]
fn text_trim_whitespace() {
    assert_eq!(text_trim("  hello  "), "hello");
    assert_eq!(text_trim("\t  world\n"), "world");
}

// Triangulate: no-op on already-trimmed string
#[test]
fn text_trim_already_trimmed() {
    assert_eq!(text_trim("hello"), "hello");
}

// Triangulate: all-whitespace → empty string
#[test]
fn text_trim_all_whitespace() {
    assert_eq!(text_trim("   "), "");
}

// ── R4.2: text_split ─────────────────────────────────────────────────────

// S4.2: splits on delimiter
#[test]
fn text_split_comma() {
    assert_eq!(
        text_split("a,b,c", ","),
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
}

// Triangulate: single element (no delimiter found)
#[test]
fn text_split_no_match() {
    assert_eq!(text_split("hello", ","), vec!["hello".to_string()]);
}

// Triangulate: empty string delimiter returns original as single element
#[test]
fn text_split_empty_delimiter() {
    assert_eq!(text_split("abc", ""), vec!["abc".to_string()]);
}

// ── R4.3: text_join ──────────────────────────────────────────────────────

// S4.3: joins with separator
#[test]
fn text_join_comma() {
    assert_eq!(text_join(&["a", "b", "c"], ","), "a,b,c");
}

// Triangulate: single element
#[test]
fn text_join_single() {
    assert_eq!(text_join(&["hello"], "-"), "hello");
}

// Triangulate: empty separator
#[test]
fn text_join_no_separator() {
    assert_eq!(text_join(&["a", "b", "c"], ""), "abc");
}

// Triangulate: empty slice → empty string
#[test]
fn text_join_empty() {
    assert_eq!(text_join(&[], ","), "");
}

// ── R4.4: text_length_graphemes ──────────────────────────────────────────

// S4.4: counts codepoints in ASCII string
#[test]
fn text_length_graphemes_ascii() {
    assert_eq!(text_length_graphemes("hello"), 5);
}

// Triangulate: multi-byte codepoints (é is 2 bytes but 1 codepoint)
#[test]
fn text_length_graphemes_multibyte() {
    // "café" = c(1) a(1) f(1) é(1) = 4 codepoints
    assert_eq!(text_length_graphemes("café"), 4);
}

// Triangulate: empty string
#[test]
fn text_length_graphemes_empty() {
    assert_eq!(text_length_graphemes(""), 0);
}

// ── R4.5: text_to_bytes ──────────────────────────────────────────────────

// S4.5: encodes ASCII to UTF-8 bytes
#[test]
fn text_to_bytes_ascii() {
    assert_eq!(text_to_bytes("hi"), vec![104u8, 105u8]);
}

// Triangulate: empty string → empty vec
#[test]
fn text_to_bytes_empty() {
    assert_eq!(text_to_bytes(""), Vec::<u8>::new());
}

// ── R4.6: text_from_bytes ────────────────────────────────────────────────

// S4.6: valid UTF-8 bytes decode to string
#[test]
fn text_from_bytes_valid() {
    assert_eq!(text_from_bytes(&[104u8, 105u8]), Ok("hi".to_string()));
}

// S4.7: invalid UTF-8 bytes return Err
#[test]
fn text_from_bytes_invalid_utf8() {
    let result = text_from_bytes(&[0xFF, 0xFE]);
    assert!(result.is_err(), "invalid UTF-8 must return Err");
}

// Triangulate: empty bytes → Ok("")
#[test]
fn text_from_bytes_empty() {
    assert_eq!(text_from_bytes(&[]), Ok(String::new()));
}

// ── R4.7: text_normalize ─────────────────────────────────────────────────

// S4.7a: NFC of already-composed ASCII is identity
#[test]
fn text_normalize_nfc_ascii_identity() {
    assert_eq!(text_normalize("hello", NormalizeForm::Nfc), "hello");
}

// S4.7b: NFD decomposes a precomposed character (é U+00E9 → e + combining acute)
#[test]
fn text_normalize_nfd_decomposes_precomposed() {
    // U+00E9 LATIN SMALL LETTER E WITH ACUTE (precomposed, 2 bytes in UTF-8)
    let precomposed = "\u{00E9}";
    let nfd = text_normalize(precomposed, NormalizeForm::Nfd);
    // NFD must produce 2 codepoints: U+0065 + U+0301
    let codepoints: Vec<char> = nfd.chars().collect();
    assert_eq!(
        codepoints.len(),
        2,
        "NFD must decompose U+00E9 into e + combining accent"
    );
    assert_eq!(codepoints[0], 'e');
    assert_eq!(codepoints[1], '\u{0301}');
}

// S4.7c: NFC recomposes a decomposed sequence (e + combining acute → é)
#[test]
fn text_normalize_nfc_recomposes_decomposed() {
    // e followed by U+0301 COMBINING ACUTE ACCENT (decomposed form)
    let decomposed = "e\u{0301}";
    let nfc = text_normalize(decomposed, NormalizeForm::Nfc);
    // NFC must produce 1 codepoint: U+00E9
    let codepoints: Vec<char> = nfc.chars().collect();
    assert_eq!(
        codepoints.len(),
        1,
        "NFC must recompose e + combining accent into U+00E9"
    );
    assert_eq!(codepoints[0], '\u{00E9}');
}

// S4.7d: NFC and NFD produce strings that are semantically equivalent
// (render the same) but byte-different for non-ASCII
#[test]
fn text_normalize_nfc_nfd_differ_for_nonascii() {
    let precomposed = "\u{00E9}"; // already NFC
    assert_ne!(
        text_normalize(precomposed, NormalizeForm::Nfc),
        text_normalize(precomposed, NormalizeForm::Nfd),
        "NFC and NFD must produce different byte sequences for U+00E9"
    );
}

// S4.7e: empty string normalizes to empty string
#[test]
fn text_normalize_empty_string() {
    assert_eq!(text_normalize("", NormalizeForm::Nfc), "");
    assert_eq!(text_normalize("", NormalizeForm::Nfd), "");
}

// ── R4.8: text_starts_with ────────────────────────────────────────────────

#[test]
fn text_starts_with_matching_prefix() {
    assert!(text_starts_with("hello world", "hello"));
}

#[test]
fn text_starts_with_non_matching_prefix() {
    assert!(!text_starts_with("hello world", "world"));
}

#[test]
fn text_starts_with_empty_prefix_always_true() {
    assert!(text_starts_with("hello", ""));
}

#[test]
fn text_starts_with_empty_string_non_empty_prefix() {
    assert!(!text_starts_with("", "hi"));
}

#[test]
fn text_starts_with_full_string_is_prefix() {
    assert!(text_starts_with("hello", "hello"));
}

// ── R4.9: text_ends_with ─────────────────────────────────────────────────

#[test]
fn text_ends_with_matching_suffix() {
    assert!(text_ends_with("hello world", "world"));
}

#[test]
fn text_ends_with_non_matching_suffix() {
    assert!(!text_ends_with("hello world", "hello"));
}

#[test]
fn text_ends_with_empty_suffix_always_true() {
    assert!(text_ends_with("hello", ""));
}

#[test]
fn text_ends_with_empty_string_non_empty_suffix() {
    assert!(!text_ends_with("", "hi"));
}

// ── R4.10: text_contains ─────────────────────────────────────────────────

#[test]
fn text_contains_substring_present() {
    assert!(text_contains("hello world", "lo wo"));
}

#[test]
fn text_contains_substring_absent() {
    assert!(!text_contains("hello world", "xyz"));
}

#[test]
fn text_contains_empty_needle_always_true() {
    assert!(text_contains("hello", ""));
}

#[test]
fn text_contains_exact_match() {
    assert!(text_contains("hello", "hello"));
}

// ── R4.11: text_replace ───────────────────────────────────────────────────

#[test]
fn text_replace_replaces_all_occurrences() {
    assert_eq!(text_replace("aabbaa", "aa", "X"), "XbbX");
}

#[test]
fn text_replace_no_match_returns_original() {
    assert_eq!(text_replace("hello", "xyz", "Y"), "hello");
}

#[test]
fn text_replace_empty_from_returns_unchanged() {
    assert_eq!(text_replace("hello", "", "X"), "hello");
}

#[test]
fn text_replace_to_empty_removes_occurrences() {
    assert_eq!(text_replace("hello world", "o", ""), "hell wrld");
}

#[test]
fn text_replace_single_occurrence() {
    assert_eq!(text_replace("foo bar baz", "bar", "qux"), "foo qux baz");
}
