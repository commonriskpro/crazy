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

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use crate::error::CliError;
use serde_json::{Value, json};

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
    validate_project_path_for_creation(path).map_err(|diagnostic| {
        CliError::Domain(format!(
            "{} {}: {}",
            diagnostic.code, diagnostic.subject, diagnostic.message
        ))
    })?;

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

fn validate_project_path_for_creation(path: &Path) -> Result<(), ProjectWorkflowDiagnostic> {
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(ProjectWorkflowDiagnostic::error(
            "project.path.traversal",
            "<redacted:project-path>",
            "project path must not contain '..' traversal segments",
        ));
    }

    Ok(())
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

// ── ProjectWorkflowDiagnostic ─────────────────────────────────────────────

/// Stable, redacted diagnostic emitted by project workflow commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectWorkflowDiagnostic {
    pub(crate) code: &'static str,
    pub(crate) severity: &'static str,
    pub(crate) subject: String,
    pub(crate) message: &'static str,
}

impl ProjectWorkflowDiagnostic {
    fn warning(code: &'static str, subject: impl Into<String>, message: &'static str) -> Self {
        Self {
            code,
            severity: "warning",
            subject: subject.into(),
            message,
        }
    }

    fn error(code: &'static str, subject: impl Into<String>, message: &'static str) -> Self {
        Self {
            code,
            severity: "error",
            subject: subject.into(),
            message,
        }
    }

    pub(crate) fn to_json(&self) -> Value {
        json!({
            "code": self.code,
            "severity": self.severity,
            "subject": self.subject,
            "message": self.message,
        })
    }
}

/// Compute the aggregate status for project workflow diagnostics.
pub(crate) fn project_workflow_diagnostic_status(
    diagnostics: &[ProjectWorkflowDiagnostic],
) -> &'static str {
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == "error")
    {
        "error"
    } else if diagnostics.is_empty() {
        "ok"
    } else {
        "warning"
    }
}

/// Inspect the project workflow layout without leaking absolute local paths.
pub(crate) fn project_workflow_diagnostics(root: &Path) -> Vec<ProjectWorkflowDiagnostic> {
    let mut diagnostics = Vec::new();

    if !root.exists() {
        diagnostics.push(ProjectWorkflowDiagnostic::error(
            "project.root.missing",
            "project-root",
            "project root does not exist",
        ));
        sort_project_workflow_diagnostics(&mut diagnostics);
        return diagnostics;
    }

    if !root.is_dir() {
        diagnostics.push(ProjectWorkflowDiagnostic::error(
            "project.root.invalid",
            "project-root",
            "project root is not a directory",
        ));
        sort_project_workflow_diagnostics(&mut diagnostics);
        return diagnostics;
    }

    let ail_dir = root.join(".ail");
    if !ail_dir.exists() {
        diagnostics.push(ProjectWorkflowDiagnostic::warning(
            "project.config.missing",
            "project-config",
            "project is not initialized; .ail/project.toml is missing",
        ));
        sort_project_workflow_diagnostics(&mut diagnostics);
        return diagnostics;
    }

    if !ail_dir.is_dir() {
        diagnostics.push(ProjectWorkflowDiagnostic::error(
            "project.workspace.invalid_layout",
            "project-workspace",
            "project workspace metadata is not a directory",
        ));
        sort_project_workflow_diagnostics(&mut diagnostics);
        return diagnostics;
    }

    for (relative, subject) in [
        ("HEAD", "project-head"),
        ("store/objects", "project-object-store"),
    ] {
        let required = ail_dir.join(relative);
        if !required.exists() {
            diagnostics.push(ProjectWorkflowDiagnostic::error(
                "project.workspace.invalid_layout",
                subject,
                "project workspace layout is missing required metadata",
            ));
        }
    }

    let config_path = ail_dir.join("project.toml");
    if !config_path.exists() {
        diagnostics.push(ProjectWorkflowDiagnostic::warning(
            "project.config.missing",
            "project-config",
            "project config is missing",
        ));
    } else if !config_path.is_file() {
        diagnostics.push(ProjectWorkflowDiagnostic::error(
            "project.workspace.invalid_layout",
            "project-config",
            "project config is not a file",
        ));
    } else {
        match std::fs::read_to_string(&config_path) {
            Ok(config) => diagnostics.extend(project_config_duplicate_diagnostics(&config)),
            Err(_) => diagnostics.push(ProjectWorkflowDiagnostic::error(
                "project.workspace.invalid_layout",
                "project-config",
                "project config could not be read",
            )),
        }
    }

    sort_project_workflow_diagnostics(&mut diagnostics);
    diagnostics
}

fn sort_project_workflow_diagnostics(diagnostics: &mut [ProjectWorkflowDiagnostic]) {
    diagnostics.sort_by(|left, right| {
        (
            left.code,
            left.severity,
            left.subject.as_str(),
            left.message,
        )
            .cmp(&(
                right.code,
                right.severity,
                right.subject.as_str(),
                right.message,
            ))
    });
}

fn project_config_duplicate_diagnostics(config: &str) -> Vec<ProjectWorkflowDiagnostic> {
    let mut modules = BTreeMap::new();
    let mut packages = BTreeMap::new();

    for line in config.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let values = quoted_values(value);
        match key {
            "module" | "modules" => {
                for value in values {
                    *modules.entry(value).or_insert(0usize) += 1;
                }
            }
            "package" | "packages" => {
                for value in values {
                    *packages.entry(value).or_insert(0usize) += 1;
                }
            }
            _ => {}
        }
    }

    let mut diagnostics = Vec::new();
    for (module, count) in modules {
        if count > 1 {
            diagnostics.push(ProjectWorkflowDiagnostic::error(
                "project.config.duplicate_module_entry",
                redacted_config_subject("module", &module),
                "project config contains a duplicate module entry",
            ));
        }
    }
    for (package, count) in packages {
        if count > 1 {
            diagnostics.push(ProjectWorkflowDiagnostic::error(
                "project.config.duplicate_package_entry",
                redacted_config_subject("package", &package),
                "project config contains a duplicate package entry",
            ));
        }
    }

    diagnostics
}

fn quoted_values(value: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut quote = '\0';
    let mut escaped = false;

    for ch in value.chars() {
        if in_quote {
            if escaped {
                current.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                values.push(std::mem::take(&mut current));
                in_quote = false;
            } else {
                current.push(ch);
            }
        } else if ch == '"' || ch == '\'' {
            in_quote = true;
            quote = ch;
        }
    }

    values
}

fn redacted_config_subject(kind: &str, value: &str) -> String {
    if value.len() <= 64
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/'))
    {
        format!("{kind}:{value}")
    } else {
        format!("{kind}:<redacted>")
    }
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

    // Scenario: project creation rejects parent-directory traversal before
    // scaffold names are derived.
    //   GIVEN a project path containing `..`
    //   WHEN scaffold names are derived
    //   THEN a stable redacted path diagnostic is returned
    #[test]
    fn project_scaffold_names_reject_path_traversal() {
        let err = project_scaffold_names(Path::new("../escape"))
            .expect_err("path traversal must not be accepted");
        let msg = err.to_string();

        assert!(
            msg.contains("project.path.traversal"),
            "diagnostic must expose stable code; got: {msg}"
        );
        assert!(
            msg.contains("<redacted:project-path>"),
            "diagnostic must redact the unsafe path; got: {msg}"
        );
        assert!(
            !msg.contains("../escape"),
            "diagnostic must not echo traversal input; got: {msg}"
        );
    }

    // Scenario: project workflow diagnostics are deterministic and redacted.
    //   GIVEN an invalid .ail layout with duplicate module/package entries
    //   WHEN diagnostics are collected
    //   THEN codes are stable sorted and duplicate entries are reported
    #[test]
    fn project_workflow_diagnostics_sort_invalid_layout_and_duplicates() {
        let root =
            std::env::temp_dir().join(format!("ail-project-diagnostics-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".ail")).expect("test .ail dir must be created");
        std::fs::write(
            root.join(".ail").join("project.toml"),
            "packages = [\"pkg.alpha\", \"pkg.alpha\"]\nmodule = \"mod.core\"\nmodule = \"mod.core\"\n",
        )
        .expect("test project config must be written");

        let diagnostics = project_workflow_diagnostics(&root);
        let codes = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>();

        assert_eq!(
            codes,
            vec![
                "project.config.duplicate_module_entry",
                "project.config.duplicate_package_entry",
                "project.workspace.invalid_layout",
                "project.workspace.invalid_layout",
            ],
            "diagnostics must have deterministic code ordering"
        );
        assert_eq!(project_workflow_diagnostic_status(&diagnostics), "error");

        let _ = std::fs::remove_dir_all(&root);
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
