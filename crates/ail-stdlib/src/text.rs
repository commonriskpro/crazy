// ── ail-stdlib::text ──────────────────────────────────────────────────────
//
// Text helpers for the AIL `std.text` module.
//
// # Unicode
//
// `text_length_graphemes` counts Unicode extended grapheme clusters using
// the `unicode-segmentation` crate, as required by `docs/stdlib.md`.
//
// `text_normalize` implements Unicode NFC normalization. Because Rust's
// stdlib does not include a normalization implementation, this module
// provides a best-effort NFC-compatible pass: it decomposes and recomposes
// codepoints using canonical equivalence rules for common Latin/extended
// ranges. For full spec compliance, callers targeting complex scripts should
// use a dedicated Unicode normalization crate at the application level.

use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

// ── NormalizeForm ─────────────────────────────────────────────────────────

/// Unicode normalization form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NormalizeForm {
    /// Canonical Decomposition, followed by Canonical Composition (NFC).
    Nfc,
    /// Canonical Decomposition (NFD).
    Nfd,
}

// ── text_trim ─────────────────────────────────────────────────────────────

/// Remove leading and trailing Unicode whitespace from `s`.
pub fn text_trim(s: &str) -> String {
    s.trim().to_string()
}

// ── text_split ────────────────────────────────────────────────────────────

/// Split `s` on every occurrence of `delimiter`, returning owned substrings.
///
/// If `delimiter` is empty, returns the original string as a single element.
pub fn text_split(s: &str, delimiter: &str) -> Vec<String> {
    if delimiter.is_empty() {
        return vec![s.to_string()];
    }
    s.split(delimiter).map(str::to_string).collect()
}

// ── text_join ─────────────────────────────────────────────────────────────

/// Join `parts` with `separator` between each pair.
pub fn text_join(parts: &[&str], separator: &str) -> String {
    parts.join(separator)
}

// ── text_normalize ────────────────────────────────────────────────────────

/// Normalize `s` according to the given Unicode normalization form.
///
/// Supports `Nfc` (Canonical Decomposition + Canonical Composition) and
/// `Nfd` (Canonical Decomposition) via the `unicode-normalization` crate.
pub fn text_normalize(s: &str, form: NormalizeForm) -> String {
    match form {
        NormalizeForm::Nfc => s.nfc().collect(),
        NormalizeForm::Nfd => s.nfd().collect(),
    }
}

// ── text_length_graphemes ─────────────────────────────────────────────────

/// Count the number of Unicode extended grapheme clusters in `s`.
///
/// Uses `unicode_segmentation::UnicodeSegmentation::graphemes` to implement
/// the canonical grapheme-cluster count required by `docs/stdlib.md`.
pub fn text_length_graphemes(s: &str) -> usize {
    s.graphemes(true).count()
}

// ── text_to_bytes ─────────────────────────────────────────────────────────

/// Encode `s` as UTF-8 bytes.
pub fn text_to_bytes(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}

// ── text_from_bytes ───────────────────────────────────────────────────────

/// Decode UTF-8 bytes into a `String`.
///
/// Returns `Ok(String)` on valid UTF-8; `Err(description)` otherwise.
/// Canonical name: `bytes_to_text` per `docs/stdlib.md`.
pub fn text_from_bytes(bytes: &[u8]) -> Result<String, String> {
    std::str::from_utf8(bytes)
        .map(str::to_string)
        .map_err(|e| e.to_string())
}

/// Alias matching the canonical `docs/stdlib.md` name `bytes_to_text`.
pub fn bytes_to_text(bytes: &[u8]) -> Result<String, String> {
    text_from_bytes(bytes)
}

// ── text_starts_with ──────────────────────────────────────────────────────

/// Return `true` if `s` begins with `prefix`.
pub fn text_starts_with(s: &str, prefix: &str) -> bool {
    s.starts_with(prefix)
}

// ── text_ends_with ────────────────────────────────────────────────────────

/// Return `true` if `s` ends with `suffix`.
pub fn text_ends_with(s: &str, suffix: &str) -> bool {
    s.ends_with(suffix)
}

// ── text_contains ─────────────────────────────────────────────────────────

/// Return `true` if `s` contains `needle` as a substring.
pub fn text_contains(s: &str, needle: &str) -> bool {
    s.contains(needle)
}

// ── text_index_of ─────────────────────────────────────────────────────────

/// Return the byte offset of the first non-overlapping `needle` occurrence.
///
/// Returns `0` for an empty needle and `-1` when `needle` is absent.
/// This matches the compiler/runtime scalar helper contract used by
/// `text.index_of` in source AIL.
pub fn text_index_of(s: &str, needle: &str) -> i64 {
    s.find(needle).map(|idx| idx as i64).unwrap_or(-1)
}

// ── text_parse_int_or ─────────────────────────────────────────────────────

/// Parse `s` as a signed 64-bit integer, returning `fallback` on invalid
/// syntax or overflow.
pub fn text_parse_int_or(s: &str, fallback: i64) -> i64 {
    s.parse::<i64>().unwrap_or(fallback)
}

// ── text_replace ──────────────────────────────────────────────────────────

/// Replace every non-overlapping occurrence of `from` in `s` with `to`.
///
/// If `from` is empty, returns `s` unchanged.
pub fn text_replace(s: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return s.to_string();
    }
    s.replace(from, to)
}
