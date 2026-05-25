// ── ail-cli::link_commands ────────────────────────────────────────────────
//
// Handler for `ail link`.
//
// Resolves a persisted native artifact by profile (via StoreHandle::load_native_artifact),
// then invokes the system linker through an injectable LinkerBoundary.
// Returns a clear Domain error when the artifact is missing or the linker is
// unavailable.
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
use crate::output::{OutputMode, print_error_response, print_response};
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
/// Resolves the latest persisted native artifact for `profile`, then links it
/// into an executable via the injectable `linker` boundary.
///
/// Default output path: `./<profile>` in the current working directory (e.g.
/// `./dev`).  Pass `--output` to override.
///
/// Errors (all `CliError::Domain`):
/// - No native artifact persisted for `profile` — suggests running `ail compile`.
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
        let msg = format!(
            "no native artifact for profile '{profile}'; \
                 run `ail compile --target native --profile {profile}` first"
        );
        if mode == OutputMode::Json {
            print_error_response(json!({
                "error": "no native artifact",
                "profile": profile,
                "message": msg,
                "next_action": format!("ail compile --target native --profile {profile}"),
            }));
        }
        CliError::Domain(msg)
    })?;

    // 2. Determine output path.
    //    Default: `./<profile>` in the current working directory (e.g. `./dev`).
    //    Explicit --output overrides.
    //
    //    TODO(W3): load_native_artifact reads object_bytes into memory even though
    //    cmd_link only needs artifact.paths.object_path.  Introduce a lighter
    //    load_native_artifact_paths() that skips the file read for link-only callers.
    let object_path = &artifact.paths.object_path;
    let output_path: PathBuf = if let Some(p) = output {
        p.to_path_buf()
    } else {
        PathBuf::from(profile)
    };

    // 3. Invoke linker through the boundary.
    let result = linker
        .link(object_path, &output_path)
        .inspect_err(|e| {
            if mode == OutputMode::Json {
                print_error_response(json!({
                    "error": "linker_failed",
                    "profile": profile,
                    "object_path": object_path.to_string_lossy(),
                    "message": e.to_string(),
                }));
            }
        })?;

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
