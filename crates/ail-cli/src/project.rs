// ── ail-cli::project ──────────────────────────────────────────────────────
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
//   native/       ← compiled native object artifacts
// ```

use std::path::{Path, PathBuf};

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
    /// Compiled native object artifact (ELF / Mach-O / COFF).
    Native,
}

// ── ProjectScaffoldNames ─────────────────────────────────────────────────

/// User-visible project name plus compiler/tooling-safe scaffold identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectScaffoldNames {
    /// Name persisted in `.ail/project.toml`.
    pub manifest_name: String,
    /// Identifier-safe prefix used by generated starter ACL/source artifacts.
    pub scaffold_ident: String,
}

/// Derive deterministic project scaffold names from a target path.
///
/// `ail new` writes the directory basename into TOML and also injects it into
/// generated starter ACL.  Keeping validation here gives project lifecycle
/// commands one shared invariant: generated projects must be manifest-safe and
/// starter files must be immediately usable by follow-up tooling.
pub(crate) fn project_scaffold_names(path: &Path) -> Result<ProjectScaffoldNames, CliError> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            CliError::Domain(format!(
                "invalid project name for {}: expected a UTF-8 directory name",
                path.display()
            ))
        })?;

    validate_project_manifest_name(name).map_err(|reason| {
        CliError::Domain(format!(
            "invalid project name '{name}': {reason}. Use ASCII letters, digits, '.', '_' or '-', starting with a letter or digit"
        ))
    })?;

    Ok(ProjectScaffoldNames {
        manifest_name: name.to_string(),
        scaffold_ident: scaffold_ident_from_project_name(name),
    })
}

fn validate_project_manifest_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("name must not be empty");
    }
    if matches!(name, "." | "..") {
        return Err("name must not be '.' or '..'");
    }
    let mut chars = name.chars();
    let first = chars
        .next()
        .expect("non-empty project names have first char");
    if !first.is_ascii_alphanumeric() {
        return Err("name must start with a letter or digit");
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err("name contains unsupported characters");
    }
    Ok(())
}

fn scaffold_ident_from_project_name(name: &str) -> String {
    let mut ident = String::new();
    if matches!(name.chars().next(), Some(ch) if ch.is_ascii_digit()) {
        ident.push_str("project_");
    }

    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            ident.push(ch);
        } else {
            ident.push('_');
        }
    }
    ident
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
            ArtifactKind::Native => "native",
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
        assert_eq!(
            ctx.artifact_name(ArtifactKind::Native, id),
            PathBuf::from("/proj/.ail/native/testid")
        );
    }

    // Scenario: project scaffold names preserve the manifest name but create
    // tooling-safe identifiers for generated starter ACL.
    //   GIVEN a project directory with package-style punctuation
    //   WHEN scaffold names are derived
    //   THEN TOML keeps the user-facing name and ACL uses an identifier-safe slug
    #[test]
    fn project_scaffold_names_sanitize_generated_identifier() {
        let names = project_scaffold_names(Path::new("my-app.v1"))
            .expect("package-style project names must be accepted");

        assert_eq!(names.manifest_name, "my-app.v1");
        assert_eq!(names.scaffold_ident, "my_app_v1");
    }

    // Scenario: numeric package-style names remain manifest-safe and get a
    // valid generated identifier prefix.
    //   GIVEN a project name that starts with a digit
    //   WHEN scaffold names are derived
    //   THEN generated ACL identifiers do not start with a digit
    #[test]
    fn project_scaffold_names_prefix_numeric_generated_identifier() {
        let names = project_scaffold_names(Path::new("2026-tool"))
            .expect("numeric-leading project names are valid manifest names");

        assert_eq!(names.manifest_name, "2026-tool");
        assert_eq!(names.scaffold_ident, "project_2026_tool");
    }

    // Scenario: unsafe names fail before project creation can write invalid TOML.
    //   GIVEN a project directory containing spaces
    //   WHEN scaffold names are derived
    //   THEN a clear domain error is returned
    #[test]
    fn project_scaffold_names_reject_unsafe_manifest_name() {
        let err = project_scaffold_names(Path::new("bad name"))
            .expect_err("spaces would make generated project metadata unsafe");
        let msg = err.to_string();

        assert!(
            msg.contains("invalid project name 'bad name'"),
            "error must name the invalid project; got: {msg}"
        );
        assert!(
            msg.contains("Use ASCII letters"),
            "error must explain the safe project-name contract; got: {msg}"
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
