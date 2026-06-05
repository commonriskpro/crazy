// ── ail-stdlib::encoding ──────────────────────────────────────────────────
//
// Base64 and hex encoding/decoding for the AIL `std.encoding` module.
//
// # Rules (from docs/stdlib.md)
//
// - decoders return Result
// - encoders declare exported fields

// ── Decode contracts ─────────────────────────────────────────────────────

/// Encoding families exposed by `std.encoding` decoders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EncodingKind {
    Base64,
    Hex,
}

impl EncodingKind {
    pub fn label(self) -> &'static str {
        match self {
            EncodingKind::Base64 => "base64",
            EncodingKind::Hex => "hex",
        }
    }
}

/// Stable redacted decode failure categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DecodeIssueKind {
    InvalidLength,
    InvalidCharacter,
}

impl DecodeIssueKind {
    pub fn code(self) -> &'static str {
        match self {
            DecodeIssueKind::InvalidLength => "ENCODING_INVALID_LENGTH",
            DecodeIssueKind::InvalidCharacter => "ENCODING_INVALID_CHARACTER",
        }
    }

    pub fn category(self) -> &'static str {
        match self {
            DecodeIssueKind::InvalidLength => "length-shape",
            DecodeIssueKind::InvalidCharacter => "character-shape",
        }
    }
}

/// Redacted decode issue suitable for LSP/logging/registry checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeIssue {
    pub encoding: EncodingKind,
    pub kind: DecodeIssueKind,
    pub code: &'static str,
    pub category: &'static str,
}

impl DecodeIssue {
    pub fn new(encoding: EncodingKind, kind: DecodeIssueKind) -> Self {
        Self {
            encoding,
            kind,
            code: kind.code(),
            category: kind.category(),
        }
    }

    pub fn diagnostic_key(&self) -> String {
        format!(
            "std.encoding.decode:{}:{}:{}",
            self.encoding.label(),
            self.category,
            self.code
        )
    }
}

// ── EncodeError / DecodeError ─────────────────────────────────────────────

/// Error produced during encoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodeError(pub String);

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "encode error: {}", self.0)
    }
}
impl std::error::Error for EncodeError {}

/// Error produced during decoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodeError(pub String);

impl DecodeError {
    pub fn issue_for(&self, encoding: EncodingKind) -> DecodeIssue {
        DecodeIssue::new(encoding, classify_decode_error(&self.0))
    }
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "decode error: {}", self.0)
    }
}
impl std::error::Error for DecodeError {}

/// Classify existing decode messages into stable redacted categories.
pub fn classify_decode_error(message: &str) -> DecodeIssueKind {
    if message.contains("length") {
        DecodeIssueKind::InvalidLength
    } else {
        DecodeIssueKind::InvalidCharacter
    }
}

// ── Base64 ────────────────────────────────────────────────────────────────

/// Encode bytes to a standard Base64 string (RFC 4648, with padding).
pub fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() {
            data[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < data.len() {
            data[i + 2] as u32
        } else {
            0
        };
        let combined = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((combined >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((combined >> 12) & 0x3F) as usize] as char);
        if i + 1 < data.len() {
            out.push(TABLE[((combined >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < data.len() {
            out.push(TABLE[(combined & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

/// Decode a standard Base64 string to bytes.
pub fn base64_decode(s: &str) -> Result<Vec<u8>, DecodeError> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a') as u32 + 26),
            b'0'..=b'9' => Some((c - b'0') as u32 + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            b'=' => None, // padding
            _ => None,
        }
    }
    let s = s.as_bytes();
    let len = s.len();
    if !len.is_multiple_of(4) {
        return Err(DecodeError("invalid base64 length".into()));
    }
    let mut out = Vec::with_capacity(len / 4 * 3);
    let mut i = 0;
    while i < len {
        let c0 = val(s[i]).ok_or_else(|| DecodeError(format!("invalid char at {i}")))?;
        let c1 = val(s[i + 1]).ok_or_else(|| DecodeError(format!("invalid char at {}", i + 1)))?;
        out.push(((c0 << 2) | (c1 >> 4)) as u8);
        if s[i + 2] != b'=' {
            let c2 =
                val(s[i + 2]).ok_or_else(|| DecodeError(format!("invalid char at {}", i + 2)))?;
            out.push(((c1 << 4) | (c2 >> 2)) as u8);
            if s[i + 3] != b'=' {
                let c3 = val(s[i + 3])
                    .ok_or_else(|| DecodeError(format!("invalid char at {}", i + 3)))?;
                out.push(((c2 << 6) | c3) as u8);
            }
        }
        i += 4;
    }
    Ok(out)
}

// ── Hex ───────────────────────────────────────────────────────────────────

/// Encode bytes to a lowercase hexadecimal string.
pub fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

/// Decode a hexadecimal string (case-insensitive) to bytes.
pub fn hex_decode(s: &str) -> Result<Vec<u8>, DecodeError> {
    if !s.len().is_multiple_of(2) {
        return Err(DecodeError("odd-length hex string".into()));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .map_err(|_| DecodeError(format!("invalid hex at {i}")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_decode_errors_have_redacted_stable_issues() {
        let err = base64_decode("secretToken").expect_err("invalid base64 length");
        let issue = err.issue_for(EncodingKind::Base64);

        assert_eq!(issue.kind, DecodeIssueKind::InvalidLength);
        assert_eq!(issue.code, "ENCODING_INVALID_LENGTH");
        assert_eq!(issue.category, "length-shape");
        assert_eq!(
            issue.diagnostic_key(),
            "std.encoding.decode:base64:length-shape:ENCODING_INVALID_LENGTH"
        );
        assert!(!issue.diagnostic_key().contains("secret"));
    }

    #[test]
    fn hex_decode_errors_have_redacted_stable_issues() {
        let odd = hex_decode("abc").expect_err("odd hex");
        assert_eq!(
            odd.issue_for(EncodingKind::Hex).diagnostic_key(),
            "std.encoding.decode:hex:length-shape:ENCODING_INVALID_LENGTH"
        );

        let bad = hex_decode("zz").expect_err("bad hex");
        assert_eq!(
            bad.issue_for(EncodingKind::Hex).diagnostic_key(),
            "std.encoding.decode:hex:character-shape:ENCODING_INVALID_CHARACTER"
        );
    }
}
