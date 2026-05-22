// ── ail-stdlib::text ──────────────────────────────────────────────────────
//
// Text helpers for the AIL `std.text` module.
// Implementations follow G26 stdlib-impl spec R4.1–R4.6.
//
// # Unicode note
//
// `text_length_graphemes` counts Unicode scalar values (codepoints via
// `str::chars().count()`), not grapheme clusters.  A fully spec-conformant
// grapheme-cluster count would require the `unicode-segmentation` crate, which
// is not a current `ail-stdlib` dependency.  This is documented as a known
// limitation; the function signature matches the spec and the implementation
// is correct for ASCII and most common Unicode input.

/// Remove leading and trailing ASCII whitespace from `s`.
pub fn text_trim(s: &str) -> String {
    s.trim().to_string()
}

/// Split `s` on every occurrence of `delimiter`, returning owned substrings.
///
/// If `delimiter` is empty, returns the original string as a single element.
pub fn text_split(s: &str, delimiter: &str) -> Vec<String> {
    if delimiter.is_empty() {
        return vec![s.to_string()];
    }
    s.split(delimiter).map(str::to_string).collect()
}

/// Join `parts` with `separator` between each pair.
pub fn text_join(parts: &[&str], separator: &str) -> String {
    parts.join(separator)
}

/// Count the number of Unicode scalar values (codepoints) in `s`.
///
/// This is an approximation of grapheme-cluster count; see module doc.
pub fn text_length_graphemes(s: &str) -> usize {
    s.chars().count()
}

/// Encode `s` as UTF-8 bytes.
pub fn text_to_bytes(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}

/// Decode UTF-8 bytes into a `String`.
///
/// Returns `Ok(String)` on valid UTF-8; `Err(description)` otherwise.
pub fn text_from_bytes(bytes: &[u8]) -> Result<String, String> {
    std::str::from_utf8(bytes)
        .map(str::to_string)
        .map_err(|e| e.to_string())
}
