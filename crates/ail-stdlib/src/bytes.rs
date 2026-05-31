// ── ail-stdlib::bytes ─────────────────────────────────────────────────────
//
// Byte buffer operations for the AIL `std.bytes` module.

// ── Byte diagnostics ─────────────────────────────────────────────────────

/// Stable byte operation names for production diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BytesOperation {
    Slice,
    ToText,
    Concat,
}

impl BytesOperation {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Slice => "slice",
            Self::ToText => "to_text",
            Self::Concat => "concat",
        }
    }
}

/// Stable byte contract issue kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BytesIssueKind {
    SliceStartOutOfBounds,
    SliceEndOutOfBounds,
    SliceStartAfterEnd,
    InvalidUtf8,
    LengthOverflow,
}

impl BytesIssueKind {
    pub const fn code(self) -> &'static str {
        match self {
            Self::SliceStartOutOfBounds => "std.bytes.slice.start_out_of_bounds",
            Self::SliceEndOutOfBounds => "std.bytes.slice.end_out_of_bounds",
            Self::SliceStartAfterEnd => "std.bytes.slice.start_after_end",
            Self::InvalidUtf8 => "std.bytes.text.invalid_utf8",
            Self::LengthOverflow => "std.bytes.length.overflow",
        }
    }

    pub const fn category(self) -> &'static str {
        match self {
            Self::SliceStartOutOfBounds | Self::SliceEndOutOfBounds | Self::SliceStartAfterEnd => {
                "bounds"
            }
            Self::InvalidUtf8 => "encoding",
            Self::LengthOverflow => "capacity",
        }
    }
}

/// Machine-readable byte issue descriptor that exposes shape, not byte contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BytesIssue {
    pub operation: BytesOperation,
    pub kind: BytesIssueKind,
    pub len: usize,
    pub start: Option<usize>,
    pub end: Option<usize>,
}

impl BytesIssue {
    pub const fn code(&self) -> &'static str {
        self.kind.code()
    }

    pub const fn category(&self) -> &'static str {
        self.kind.category()
    }

    pub const fn operation_label(&self) -> &'static str {
        self.operation.label()
    }

    pub fn diagnostic_key(&self) -> String {
        format!(
            "std.bytes:{}:{}:{}",
            self.operation_label(),
            self.category(),
            self.code()
        )
    }
}

fn bytes_issue(
    operation: BytesOperation,
    kind: BytesIssueKind,
    len: usize,
    start: Option<usize>,
    end: Option<usize>,
) -> BytesIssue {
    BytesIssue {
        operation,
        kind,
        len,
        start,
        end,
    }
}

/// Error that preserves stable byte diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BytesError {
    pub issue: BytesIssue,
}

impl BytesError {
    pub const fn code(&self) -> &'static str {
        self.issue.code()
    }
}

impl std::fmt::Display for BytesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bytes error: {}", self.issue.code())
    }
}

impl std::error::Error for BytesError {}

/// A byte buffer (owned `Vec<u8>`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Bytes(pub Vec<u8>);

impl Bytes {
    /// Construct an empty byte buffer.
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Construct from a raw `Vec<u8>`.
    pub fn from_vec(v: Vec<u8>) -> Self {
        Self(v)
    }

    /// Construct from a slice.
    pub fn from_slice(s: &[u8]) -> Self {
        Self(s.to_vec())
    }

    /// Return the length in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Return `true` if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Return a reference to the raw bytes.
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// Append another `Bytes` buffer, returning a new buffer.
    pub fn concat(&self, other: &Bytes) -> Bytes {
        self.try_concat(other)
            .expect("byte buffer length must fit in usize")
    }

    /// Append another `Bytes` buffer with length-overflow diagnostics.
    pub fn try_concat(&self, other: &Bytes) -> Result<Bytes, BytesError> {
        let total_len = self.len().checked_add(other.len()).ok_or(BytesError {
            issue: bytes_issue(
                BytesOperation::Concat,
                BytesIssueKind::LengthOverflow,
                self.len(),
                None,
                None,
            ),
        })?;
        let mut v = Vec::with_capacity(total_len);
        v.extend_from_slice(&self.0);
        v.extend_from_slice(&other.0);
        Ok(Bytes(v))
    }

    /// Slice the buffer `[start..end]`, returning `None` if out of bounds.
    pub fn slice(&self, start: usize, end: usize) -> Option<Bytes> {
        self.try_slice(start, end).ok()
    }

    /// Slice the buffer `[start..end]` with stable, redacted bounds diagnostics.
    pub fn try_slice(&self, start: usize, end: usize) -> Result<Bytes, BytesError> {
        if start > end {
            return Err(BytesError {
                issue: bytes_issue(
                    BytesOperation::Slice,
                    BytesIssueKind::SliceStartAfterEnd,
                    self.len(),
                    Some(start),
                    Some(end),
                ),
            });
        }
        if start > self.len() {
            return Err(BytesError {
                issue: bytes_issue(
                    BytesOperation::Slice,
                    BytesIssueKind::SliceStartOutOfBounds,
                    self.len(),
                    Some(start),
                    Some(end),
                ),
            });
        }
        if end > self.len() {
            return Err(BytesError {
                issue: bytes_issue(
                    BytesOperation::Slice,
                    BytesIssueKind::SliceEndOutOfBounds,
                    self.len(),
                    Some(start),
                    Some(end),
                ),
            });
        }
        Ok(Bytes(self.0[start..end].to_vec()))
    }

    /// Decode to a UTF-8 string.
    ///
    /// Returns `Ok(String)` on valid UTF-8; `Err(description)` otherwise.
    pub fn to_text(&self) -> Result<String, String> {
        self.try_to_text().map_err(|e| e.to_string())
    }

    /// Decode to UTF-8 text with stable, redacted diagnostics.
    pub fn try_to_text(&self) -> Result<String, BytesError> {
        std::str::from_utf8(&self.0)
            .map(str::to_string)
            .map_err(|_| BytesError {
                issue: bytes_issue(
                    BytesOperation::ToText,
                    BytesIssueKind::InvalidUtf8,
                    self.len(),
                    None,
                    None,
                ),
            })
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(v: Vec<u8>) -> Self {
        Bytes(v)
    }
}

impl From<&[u8]> for Bytes {
    fn from(s: &[u8]) -> Self {
        Bytes(s.to_vec())
    }
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_slice_reports_bounds_without_bytes() {
        let bytes = Bytes::from_slice(b"secret-token");

        let issue = bytes.try_slice(2, 99).unwrap_err().issue;

        assert_eq!(issue.code(), "std.bytes.slice.end_out_of_bounds");
        assert_eq!(issue.category(), "bounds");
        assert_eq!(issue.operation_label(), "slice");
        assert_eq!(issue.len, 12);
        assert_eq!(issue.start, Some(2));
        assert_eq!(issue.end, Some(99));
        assert!(!issue.diagnostic_key().contains("secret"));
    }

    #[test]
    fn try_slice_reports_reversed_range_first() {
        let bytes = Bytes::from_slice(b"abcdef");

        let issue = bytes.try_slice(5, 2).unwrap_err().issue;

        assert_eq!(issue.code(), "std.bytes.slice.start_after_end");
        assert_eq!(issue.category(), "bounds");
        assert_eq!(issue.len, 6);
        assert_eq!(issue.start, Some(5));
        assert_eq!(issue.end, Some(2));
    }

    #[test]
    fn try_to_text_reports_invalid_utf8_without_payload() {
        let bytes = Bytes::from_vec(vec![0xff, 0xfe, b's', b'e', b'c', b'r', b'e', b't']);

        let err = bytes.try_to_text().unwrap_err();

        assert_eq!(err.code(), "std.bytes.text.invalid_utf8");
        assert_eq!(err.issue.category(), "encoding");
        assert_eq!(err.issue.operation_label(), "to_text");
        assert_eq!(err.issue.len, 8);
        assert!(!err.issue.diagnostic_key().contains("secret"));
    }

    #[test]
    fn legacy_slice_and_to_text_stay_compatible() {
        let bytes = Bytes::from_slice(b"hello");

        assert_eq!(bytes.slice(1, 4), Some(Bytes::from_slice(b"ell")));
        assert_eq!(bytes.slice(4, 99), None);
        assert_eq!(bytes.to_text(), Ok("hello".to_string()));
    }
}
