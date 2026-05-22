// Tests for ail-stdlib::text — text helpers.
//
// TDD cycle: tests written before implementation.
// Spec: G26 stdlib-impl, Requirements R4.1–R4.6.

use ail_stdlib::text::{
    text_from_bytes, text_join, text_length_graphemes, text_split, text_to_bytes, text_trim,
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
