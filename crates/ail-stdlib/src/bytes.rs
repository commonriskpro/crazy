// ── ail-stdlib::bytes ─────────────────────────────────────────────────────
//
// Byte buffer operations for the AIL `std.bytes` module.

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
        let mut v = self.0.clone();
        v.extend_from_slice(&other.0);
        Bytes(v)
    }

    /// Slice the buffer `[start..end]`, returning `None` if out of bounds.
    pub fn slice(&self, start: usize, end: usize) -> Option<Bytes> {
        self.0.get(start..end).map(|s| Bytes(s.to_vec()))
    }

    /// Decode to a UTF-8 string.
    ///
    /// Returns `Ok(String)` on valid UTF-8; `Err(description)` otherwise.
    pub fn to_text(&self) -> Result<String, String> {
        std::str::from_utf8(&self.0)
            .map(str::to_string)
            .map_err(|e| e.to_string())
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
