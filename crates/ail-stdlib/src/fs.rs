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

impl FsCapability {
    /// Stable runtime capability label required by the host boundary.
    pub fn label(self) -> &'static str {
        match self {
            FsCapability::Read => "file.read",
            FsCapability::Write => "file.write",
            FsCapability::Delete => "file.delete",
            FsCapability::List => "file.list",
        }
    }
}

impl std::fmt::Display for FsCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

// ── Operation descriptors ─────────────────────────────────────────────────

/// Stable filesystem operation categories used by diagnostics and registries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsOperationKind {
    ReadFile,
    WriteFile,
    DeletePath,
    ListDirectory,
    ReadMetadata,
}

impl FsOperationKind {
    /// Deterministic operation label for diagnostics and stdlib registries.
    pub fn label(self) -> &'static str {
        match self {
            FsOperationKind::ReadFile => "read_file",
            FsOperationKind::WriteFile => "write_file",
            FsOperationKind::DeletePath => "delete_path",
            FsOperationKind::ListDirectory => "list_directory",
            FsOperationKind::ReadMetadata => "read_metadata",
        }
    }

    /// Capability that must be granted before the operation can run.
    pub fn required_capability(self) -> FsCapability {
        match self {
            FsOperationKind::ReadFile | FsOperationKind::ReadMetadata => FsCapability::Read,
            FsOperationKind::WriteFile => FsCapability::Write,
            FsOperationKind::DeletePath => FsCapability::Delete,
            FsOperationKind::ListDirectory => FsCapability::List,
        }
    }
}

/// Stable shape categories for user-provided filesystem paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsPathShape {
    Empty,
    Root,
    Absolute,
    Relative,
    ContainsParentTraversal,
    ContainsNul,
}

impl FsPathShape {
    /// Diagnostic label that does not leak the path itself.
    pub fn label(self) -> &'static str {
        match self {
            FsPathShape::Empty => "empty",
            FsPathShape::Root => "root",
            FsPathShape::Absolute => "absolute",
            FsPathShape::Relative => "relative",
            FsPathShape::ContainsParentTraversal => "contains-parent-traversal",
            FsPathShape::ContainsNul => "contains-nul",
        }
    }

    /// Whether this shape is structurally acceptable before capability checks.
    pub fn is_allowed(self) -> bool {
        matches!(
            self,
            FsPathShape::Root | FsPathShape::Absolute | FsPathShape::Relative
        )
    }
}

/// Error produced when a path has an unsupported structural shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsPathShapeError {
    pub shape: FsPathShape,
    pub expected: &'static str,
}

impl std::fmt::Display for FsPathShapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "fs path shape mismatch: expected {}, got {}",
            self.expected,
            self.shape.label()
        )
    }
}

impl std::error::Error for FsPathShapeError {}

/// Descriptor proving std.fs operations are capability-gated, not ambient.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsOperationDescriptor {
    pub operation: FsOperationKind,
    pub capability: FsCapability,
    pub capability_label: &'static str,
    pub path_shape: FsPathShape,
    pub grant_required: bool,
    pub ambient_access: bool,
}

impl FsOperationDescriptor {
    /// Build a descriptor for an operation/path pair without granting access.
    pub fn new(operation: FsOperationKind, path: &Path) -> Self {
        let capability = operation.required_capability();
        Self {
            operation,
            capability,
            capability_label: capability.label(),
            path_shape: path_shape(path),
            grant_required: true,
            ambient_access: false,
        }
    }

    /// Deterministic low-cardinality descriptor suitable for logs/registries.
    pub fn diagnostic_key(&self) -> String {
        format!(
            "std.fs.{}:{}:{}",
            self.operation.label(),
            self.capability_label,
            self.path_shape.label()
        )
    }

    /// Validate the path shape before any host filesystem operation runs.
    pub fn validate_path_shape(&self) -> Result<(), FsPathShapeError> {
        validate_path_shape(self.path_shape)
    }
}

/// Return the stable structural shape for a path without exposing its value.
pub fn path_shape(path: &Path) -> FsPathShape {
    let raw = path.as_str();
    if raw.is_empty() {
        return FsPathShape::Empty;
    }
    if raw.contains('\0') {
        return FsPathShape::ContainsNul;
    }
    if raw.split('/').any(|segment| segment == "..") {
        return FsPathShape::ContainsParentTraversal;
    }
    if raw == "/" {
        return FsPathShape::Root;
    }
    if raw.starts_with('/') {
        return FsPathShape::Absolute;
    }
    FsPathShape::Relative
}

/// Validate an already-classified path shape.
pub fn validate_path_shape(shape: FsPathShape) -> Result<(), FsPathShapeError> {
    if shape.is_allowed() {
        Ok(())
    } else {
        Err(FsPathShapeError {
            shape,
            expected: "root|absolute|relative without parent traversal or NUL",
        })
    }
}

/// Metadata about a filesystem entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMetadata {
    pub path: Path,
    pub size_bytes: u64,
    pub is_file: bool,
    pub is_dir: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fs_capability_labels_are_stable() {
        assert_eq!(FsCapability::Read.label(), "file.read");
        assert_eq!(FsCapability::Write.label(), "file.write");
        assert_eq!(FsCapability::Delete.label(), "file.delete");
        assert_eq!(FsCapability::List.label(), "file.list");
    }

    #[test]
    fn operation_descriptors_make_denied_by_default_semantics_visible() {
        let read = FsOperationDescriptor::new(FsOperationKind::ReadFile, &Path::new("data/app.db"));
        let write =
            FsOperationDescriptor::new(FsOperationKind::WriteFile, &Path::new("/var/app.log"));
        let list = FsOperationDescriptor::new(FsOperationKind::ListDirectory, &Path::new("/"));

        assert_eq!(read.capability, FsCapability::Read);
        assert_eq!(read.capability_label, "file.read");
        assert_eq!(read.path_shape, FsPathShape::Relative);
        assert!(read.grant_required);
        assert!(!read.ambient_access);

        assert_eq!(write.capability_label, "file.write");
        assert_eq!(write.path_shape, FsPathShape::Absolute);
        assert_eq!(
            write.diagnostic_key(),
            "std.fs.write_file:file.write:absolute"
        );

        assert_eq!(list.capability_label, "file.list");
        assert_eq!(list.path_shape, FsPathShape::Root);
        assert_eq!(list.validate_path_shape(), Ok(()));
    }

    #[test]
    fn path_shape_validation_rejects_unsafe_shapes_without_leaking_path() {
        let parent =
            FsOperationDescriptor::new(FsOperationKind::ReadFile, &Path::new("config/../secret"));
        let nul = FsOperationDescriptor::new(FsOperationKind::WriteFile, &Path::new("cache/\0tmp"));
        let empty = FsOperationDescriptor::new(FsOperationKind::ReadMetadata, &Path::new(""));

        assert_eq!(
            parent.validate_path_shape(),
            Err(FsPathShapeError {
                shape: FsPathShape::ContainsParentTraversal,
                expected: "root|absolute|relative without parent traversal or NUL",
            })
        );
        assert_eq!(
            parent.diagnostic_key(),
            "std.fs.read_file:file.read:contains-parent-traversal"
        );
        assert_eq!(
            nul.diagnostic_key(),
            "std.fs.write_file:file.write:contains-nul"
        );
        assert_eq!(
            empty.diagnostic_key(),
            "std.fs.read_metadata:file.read:empty"
        );
    }
}
