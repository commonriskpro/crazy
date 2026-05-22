// ── ail-stdlib::fs ────────────────────────────────────────────────────────
//
// Filesystem types and operation descriptors for the AIL `std.fs` module.
//
// # Capabilities (from docs/stdlib.md)
//
// - file.read
// - file.write
// - file.delete
// - file.list
//
// # Rules
//
// - file access requires grants
// - paths/scopes are capability-constrained
// - handles use Handle<Resource, Mode>

// ── Path ──────────────────────────────────────────────────────────────────

/// A validated filesystem path.
///
/// In the AIL model, a `Path` is always capability-scoped at runtime; the
/// type alone does not grant access.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Path(pub String);

impl Path {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Join a child segment to this path.
    pub fn join(&self, child: &str) -> Self {
        let base = self.0.trim_end_matches('/');
        Self(format!("{base}/{child}"))
    }

    /// Return the file name (last segment).
    pub fn file_name(&self) -> Option<&str> {
        self.0.rsplit('/').next().filter(|s| !s.is_empty())
    }

    /// Return the parent directory.
    pub fn parent(&self) -> Option<Path> {
        let base = self.0.trim_end_matches('/');
        base.rsplit_once('/').map(|(p, _)| Path(p.to_string()))
    }
}

impl std::fmt::Display for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── FileError ─────────────────────────────────────────────────────────────

/// Error from filesystem operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileError {
    NotFound(Path),
    PermissionDenied(Path),
    AlreadyExists(Path),
    IsDirectory(Path),
    Other(String),
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileError::NotFound(p) => write!(f, "not found: {p}"),
            FileError::PermissionDenied(p) => write!(f, "permission denied: {p}"),
            FileError::AlreadyExists(p) => write!(f, "already exists: {p}"),
            FileError::IsDirectory(p) => write!(f, "is a directory: {p}"),
            FileError::Other(msg) => write!(f, "fs error: {msg}"),
        }
    }
}
impl std::error::Error for FileError {}

// ── FileHandle / DirectoryHandle ──────────────────────────────────────────

/// A file resource marker for use with `io::Handle`.
#[derive(Debug)]
pub struct FileResource {
    pub path: Path,
}

/// A directory resource marker.
#[derive(Debug)]
pub struct DirectoryResource {
    pub path: Path,
}

// ── FsCapability ──────────────────────────────────────────────────────────

/// Enumeration of filesystem capabilities required by operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsCapability {
    Read,
    Write,
    Delete,
    List,
}

/// Metadata about a filesystem entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMetadata {
    pub path: Path,
    pub size_bytes: u64,
    pub is_file: bool,
    pub is_dir: bool,
}
