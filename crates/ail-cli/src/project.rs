// ── ail-cli::project ──────────────────────────────────────────────────────
// `ProjectContext` is wired into cli.rs command handlers in PR2.
#![allow(dead_code)]
//
// `ProjectContext` owns project-local path resolution for the `.ail/`
// directory layout.  All path operations are pure: no I/O is performed here
// beyond `std::env::current_dir()` in `from_cwd()`.
//
// # `.ail/` layout
//
// ```
// <root>/.ail/
//   changes/      ← serialized ChangeSets (CBOR)
//   snapshots/    ← SnapshotEnvelopes (CBOR)
//   reports/      ← VerificationReports (CBOR)
//   wasm/         ← compiled WASM artifacts
// ```

use std::path::PathBuf;

use crate::error::CliError;

// ── ArtifactKind ──────────────────────────────────────────────────────────

/// Classifier for the type of artifact stored in the local `.ail/` directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    /// Serialized `CanonicalChangeSet` blob.
    Change,
    /// Serialized `SnapshotEnvelope` blob.
    Snapshot,
    /// Serialized `VerificationReport` blob.
    Report,
    /// Compiled WASM artifact.
    Wasm,
}

// ── ProjectContext ────────────────────────────────────────────────────────

/// Resolved project context for all CLI operations.
///
/// `root` is the project root directory.
/// `ail_dir` is `root/.ail/`, the local artifact store.
#[derive(Debug, Clone)]
pub struct ProjectContext {
    /// Project root directory (typically the working directory).
    pub root: PathBuf,
    /// Local artifact store at `root/.ail/`.
    pub ail_dir: PathBuf,
}

impl ProjectContext {
    /// Construct a `ProjectContext` from an explicit `root` path.
    ///
    /// The `ail_dir` is always derived as `root.join(".ail")`.
    /// No I/O is performed.
    pub fn new(root: PathBuf) -> Self {
        let ail_dir = root.join(".ail");
        Self { root, ail_dir }
    }

    /// Construct a `ProjectContext` from the current working directory.
    ///
    /// Returns `Err(CliError::Io(_))` if `std::env::current_dir()` fails.
    pub fn from_cwd() -> Result<Self, CliError> {
        let root = std::env::current_dir()?;
        Ok(Self::new(root))
    }

    /// Resolve the file path for an artifact of the given kind and identity.
    ///
    /// The returned path is `ail_dir/<subdir>/<id>` where `subdir` matches
    /// the `ArtifactKind`.  The path is not created on disk.
    pub fn artifact_name(&self, kind: ArtifactKind, id: &str) -> PathBuf {
        let subdir = match kind {
            ArtifactKind::Change => "changes",
            ArtifactKind::Snapshot => "snapshots",
            ArtifactKind::Report => "reports",
            ArtifactKind::Wasm => "wasm",
        };
        self.ail_dir.join(subdir).join(id)
    }
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Scenario: ail_dir is always root + ".ail".
    //   GIVEN a root path
    //   WHEN ProjectContext::new(root) is called
    //   THEN ail_dir equals root.join(".ail")
    #[test]
    fn ail_dir_is_root_dot_ail() {
        let ctx = ProjectContext::new(PathBuf::from("/tmp/myproject"));
        assert_eq!(
            ctx.ail_dir,
            PathBuf::from("/tmp/myproject/.ail"),
            "ail_dir must be root/.ail"
        );
    }

    // TRIANGULATE: artifact_name for Change kind produces changes/<id> path.
    //   GIVEN a ProjectContext with a known root
    //   WHEN artifact_name(Change, "abc123") is called
    //   THEN the result is root/.ail/changes/abc123
    #[test]
    fn artifact_name_change_is_under_changes_subdir() {
        let ctx = ProjectContext::new(PathBuf::from("/proj"));
        let path = ctx.artifact_name(ArtifactKind::Change, "abc123");
        assert_eq!(
            path,
            PathBuf::from("/proj/.ail/changes/abc123"),
            "Change artifact must be under .ail/changes/"
        );
    }

    // TRIANGULATE: artifact_name for each ArtifactKind uses the correct subdir.
    //   GIVEN a ProjectContext with a known root
    //   WHEN artifact_name is called for each ArtifactKind
    //   THEN each result is under the correct subdir
    #[test]
    fn artifact_name_subdirs_match_kind() {
        let ctx = ProjectContext::new(PathBuf::from("/proj"));
        let id = "testid";

        assert_eq!(
            ctx.artifact_name(ArtifactKind::Change, id),
            PathBuf::from("/proj/.ail/changes/testid")
        );
        assert_eq!(
            ctx.artifact_name(ArtifactKind::Snapshot, id),
            PathBuf::from("/proj/.ail/snapshots/testid")
        );
        assert_eq!(
            ctx.artifact_name(ArtifactKind::Report, id),
            PathBuf::from("/proj/.ail/reports/testid")
        );
        assert_eq!(
            ctx.artifact_name(ArtifactKind::Wasm, id),
            PathBuf::from("/proj/.ail/wasm/testid")
        );
    }

    // TRIANGULATE: from_cwd() succeeds in the current process environment.
    //   GIVEN the current working directory is accessible
    //   WHEN from_cwd() is called
    //   THEN the returned context has a non-empty root and ail_dir = root/.ail
    #[test]
    fn from_cwd_returns_valid_context() {
        let ctx = ProjectContext::from_cwd().expect("from_cwd must succeed in a test environment");
        assert!(
            ctx.root.to_str().map(|s| !s.is_empty()).unwrap_or(false),
            "root must be a non-empty path"
        );
        assert_eq!(
            ctx.ail_dir,
            ctx.root.join(".ail"),
            "ail_dir must be root/.ail"
        );
    }
}
