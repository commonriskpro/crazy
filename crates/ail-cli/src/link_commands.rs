// ── ail-cli::link_commands ────────────────────────────────────────────────
//
// Handler for `ail link`.
//
// Resolves a persisted native artifact by profile (via StoreHandle::load_native_artifact),
// validates the object path exists on disk, then invokes the system linker
// through an injectable LinkerBoundary.  Returns a clear Domain error when
// the artifact is missing or the linker is unavailable.
//
// Types:
//   LinkerBoundary           — injectable trait for linker invocation
//   LinkerResult             — outcome of a successful linker invocation
//   SystemLinker             — real impl: `cc` / `link.exe` via std::process::Command
//
// Pure helpers (no I/O, testable without a system linker):
//   detect_system_linker_cmd — platform-specific linker binary name
//   build_linker_args        — construct argument vector for object + output paths

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::error::CliError;
use crate::output::{OutputMode, print_response};
use crate::store::StoreHandle;

// ── LinkerBoundary ────────────────────────────────────────────────────────

/// The outcome of a successful linker invocation.
pub(crate) struct LinkerResult {
    /// Full command string passed to the OS (for reporting and diagnostics).
    pub command: String,
    /// Output executable path produced by the linker.
    pub output_path: PathBuf,
}

/// Injectable boundary for system linker invocation.
///
/// `SystemLinker` invokes the real platform binary (`cc` on Unix,
/// `link.exe` on Windows).  Tests inject a `FakeLinker` to verify command
/// construction and error paths without requiring a system C compiler.
pub(crate) trait LinkerBoundary {
    /// Attempt to link `object_path` into `output_path`.
    ///
    /// Returns `Ok(LinkerResult)` on success, or `CliError::Domain` when the
    /// linker is unavailable or exits with a non-zero status.
    fn link(&self, object_path: &Path, output_path: &Path) -> Result<LinkerResult, CliError>;
}

// ── Pure helpers ──────────────────────────────────────────────────────────

/// Detect the platform system linker command name.
///
/// - macOS / Linux / other Unix → `"cc"` (resolves to clang or gcc)
/// - Windows                    → `"link.exe"`
pub(crate) fn detect_system_linker_cmd() -> &'static str {
    if cfg!(target_os = "windows") {
        "link.exe"
    } else {
        "cc"
    }
}

/// Build the linker argument list for the given object and output paths.
///
/// Unix  (`cc`):          `[<object_path>, "-o", <output_path>]`
/// Windows (`link.exe`):  `[<object_path>, "/OUT:<output_path>"]`
///
/// Returns `Vec<String>` so callers can inspect the arguments in tests
/// without invoking a real linker.
pub(crate) fn build_linker_args(object_path: &Path, output_path: &Path) -> Vec<String> {
    let obj = object_path.to_string_lossy().into_owned();
    let out = output_path.to_string_lossy().into_owned();
    if cfg!(target_os = "windows") {
        vec![obj, format!("/OUT:{out}")]
    } else {
        vec![obj, "-o".to_owned(), out]
    }
}

// ── SystemLinker ──────────────────────────────────────────────────────────

/// System linker: invokes `cc` / `link.exe` via `std::process::Command`.
pub(crate) struct SystemLinker;

impl LinkerBoundary for SystemLinker {
    fn link(&self, object_path: &Path, output_path: &Path) -> Result<LinkerResult, CliError> {
        let linker_cmd = detect_system_linker_cmd();
        let args = build_linker_args(object_path, output_path);
        let command_str = format!("{linker_cmd} {}", args.join(" "));

        let status = std::process::Command::new(linker_cmd)
            .args(&args)
            .status()
            .map_err(|e| {
                CliError::Domain(format!(
                    "linker '{linker_cmd}' unavailable: {e}; \
                     install a system C compiler (cc/clang/gcc) to use `ail link`"
                ))
            })?;

        if !status.success() {
            let code = status.code().unwrap_or(-1);
            return Err(CliError::Domain(format!(
                "linker exited with code {code}; command: {command_str}"
            )));
        }

        Ok(LinkerResult {
            command: command_str,
            output_path: output_path.to_path_buf(),
        })
    }
}

// ── Command handler ───────────────────────────────────────────────────────

/// `ail link --profile <name> [--output <path>]`
///
/// Resolves the latest persisted native artifact for `profile`, validates the
/// object file is present on disk, then links it into an executable via the
/// injectable `linker` boundary.
///
/// Errors (all `CliError::Domain`):
/// - No native artifact persisted for `profile` — suggests running `ail compile`.
/// - Object file missing from disk (index is stale) — suggests re-compiling.
/// - System linker unavailable or exited non-zero.
pub(crate) fn cmd_link(
    mode: OutputMode,
    profile: &str,
    output: Option<&Path>,
    store: &StoreHandle,
    linker: &dyn LinkerBoundary,
) -> Result<(), CliError> {
    // 1. Resolve native artifact by profile.
    let artifact = store.load_native_artifact(profile)?.ok_or_else(|| {
        CliError::Domain(format!(
            "no native artifact for profile '{profile}'; \
                 run `ail compile --target native --profile {profile}` first"
        ))
    })?;

    // 2. Validate the object file is present on disk.
    let object_path = &artifact.paths.object_path;
    if !object_path.exists() {
        return Err(CliError::Domain(format!(
            "native object file not found: {}; \
             re-run `ail compile --target native --profile {profile}`",
            object_path.display()
        )));
    }

    // 3. Determine output path (strip `.o` extension from object path by default).
    let output_path: PathBuf = if let Some(p) = output {
        p.to_path_buf()
    } else {
        object_path.with_extension("")
    };

    // 4. Invoke linker through the boundary.
    let result = linker.link(object_path, &output_path)?;

    let human_msg = format!(
        "profile: {profile}\nobject: {}\noutput: {}\nlinker_command: {}\nstatus: linked",
        object_path.display(),
        output_path.display(),
        result.command,
    );
    print_response(
        mode,
        &human_msg,
        json!({
            "profile": profile,
            "object_path": object_path.to_string_lossy(),
            "output_path": result.output_path.to_string_lossy(),
            "linker_command": result.command,
            "status": "linked",
        }),
    );
    Ok(())
}
