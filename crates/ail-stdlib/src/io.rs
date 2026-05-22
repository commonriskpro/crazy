// ── ail-stdlib::io ────────────────────────────────────────────────────────
//
// Generic I/O traits and types for the AIL `std.io` module.
//
// # Rules (from docs/stdlib.md)
//
// - file access requires grants
// - handles use Handle<Resource, Mode>

// ── IoError ───────────────────────────────────────────────────────────────

/// Error from I/O operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IoError {
    /// Permission denied (capability not granted).
    PermissionDenied,
    /// Resource not found.
    NotFound,
    /// An unexpected I/O error with a description.
    Other(String),
}

impl std::fmt::Display for IoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IoError::PermissionDenied => write!(f, "permission denied"),
            IoError::NotFound => write!(f, "not found"),
            IoError::Other(msg) => write!(f, "io error: {msg}"),
        }
    }
}
impl std::error::Error for IoError {}

// ── Mode ──────────────────────────────────────────────────────────────────

/// Access mode for a handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Read,
    Write,
    ReadWrite,
    Append,
}

// ── Handle ────────────────────────────────────────────────────────────────

/// A typed handle to a resource with an associated access mode.
///
/// `Resource` is a marker type (e.g., `FileResource`, `StreamResource`).
/// Capability check happens at the boundary; this type records the intent.
#[derive(Debug)]
pub struct Handle<Resource, const MODE: u8> {
    pub resource: Resource,
    pub mode: Mode,
    _phantom: std::marker::PhantomData<Resource>,
}

impl<Resource, const MODE: u8> Handle<Resource, MODE> {
    pub fn new(resource: Resource, mode: Mode) -> Self {
        Self {
            resource,
            mode,
            _phantom: std::marker::PhantomData,
        }
    }
}

// ── Reader / Writer traits ────────────────────────────────────────────────

/// Trait for readable I/O sources.
pub trait Reader {
    /// Read up to `buf.len()` bytes into `buf`. Returns bytes read or `IoError`.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError>;

    /// Read all available bytes.
    fn read_all(&mut self) -> Result<Vec<u8>, IoError> {
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match self.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }
}

/// Trait for writable I/O sinks.
pub trait Writer {
    /// Write `buf` bytes. Returns bytes written or `IoError`.
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError>;

    /// Flush buffered output.
    fn flush(&mut self) -> Result<(), IoError>;
}

// ── InMemoryStream ────────────────────────────────────────────────────────

/// An in-memory byte stream useful for tests and boundary adapters.
#[derive(Default, Debug)]
pub struct InMemoryStream {
    data: Vec<u8>,
    pos: usize,
}

impl InMemoryStream {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn from_bytes(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }
    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }
}

impl Reader for InMemoryStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        let remaining = &self.data[self.pos..];
        let n = buf.len().min(remaining.len());
        buf[..n].copy_from_slice(&remaining[..n]);
        self.pos += n;
        Ok(n)
    }
}

impl Writer for InMemoryStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        self.data.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> Result<(), IoError> {
        Ok(())
    }
}
