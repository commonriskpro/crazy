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
//                             + optional runtime library archive
//
// Runtime library:
//   The native object imports three unresolved symbols at link time:
//     host_call        — capability dispatch (ail-runtime host boundary)
//     __ail_malloc     — heap allocator stub (ail-runtime allocator)
//     ail_runtime_call — concurrency / resource / channel dispatch (Phase 9+)
//   Pass --runtime-lib <path-to-ail_runtime.a> so the system linker can
//   resolve these at link time and produce a self-contained executable.
//   Without --runtime-lib the linker will fail with "undefined symbol" errors
//   unless you supply the symbols through another mechanism (e.g. a custom
//   link script or manual -L/-l flags).  `ail link` surfaces a hint in that case.

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
    /// `runtime_lib` — optional path to a runtime archive (e.g. `ail_runtime.a`)
    /// that provides `host_call`, `__ail_malloc`, and `ail_runtime_call`.
    /// When `Some`, the archive path is appended to the linker argument list.
    /// When `None`, the object is linked as-is; the system linker will fail
    /// with "undefined symbol" errors if those symbols are not supplied elsewhere.
    ///
    /// Returns `Ok(LinkerResult)` on success, or `CliError::Domain` when the
    /// linker is unavailable or exits with a non-zero status.
    fn link(
        &self,
        object_path: &Path,
        output_path: &Path,
        runtime_lib: Option<&Path>,
    ) -> Result<LinkerResult, CliError>;
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

/// Build the linker argument list for the given object, output, and optional runtime library.
///
/// Unix  (`cc`):
///   without runtime_lib: `[<object_path>, "-o", <output_path>]`
///   with    runtime_lib: `[<object_path>, "-o", <output_path>, <runtime_lib>]`
///
/// Windows (`link.exe`):
///   without runtime_lib: `[<object_path>, "/OUT:<output_path>"]`
///   with    runtime_lib: `[<object_path>, "/OUT:<output_path>", <runtime_lib>]`
///
/// The runtime library archive (e.g. `ail_runtime.a`) resolves the unresolved
/// imports emitted by the native backend: `host_call`, `__ail_malloc`, and
/// `ail_runtime_call`.
///
/// Returns `Vec<String>` so callers can inspect the arguments in tests
/// without invoking a real linker.
pub(crate) fn build_linker_args(
    object_path: &Path,
    output_path: &Path,
    runtime_lib: Option<&Path>,
) -> Vec<String> {
    let obj = object_path.to_string_lossy().into_owned();
    let out = output_path.to_string_lossy().into_owned();
    let mut args = if cfg!(target_os = "windows") {
        vec![obj, format!("/OUT:{out}")]
    } else {
        vec![obj, "-o".to_owned(), out]
    };
    if let Some(lib) = runtime_lib {
        args.push(lib.to_string_lossy().into_owned());
    }
    args
}

// ── SystemLinker ──────────────────────────────────────────────────────────

/// System linker: invokes `cc` / `link.exe` via `std::process::Command`.
pub(crate) struct SystemLinker;

impl LinkerBoundary for SystemLinker {
    fn link(
        &self,
        object_path: &Path,
        output_path: &Path,
        runtime_lib: Option<&Path>,
    ) -> Result<LinkerResult, CliError> {
        let linker_cmd = detect_system_linker_cmd();
        let args = build_linker_args(object_path, output_path, runtime_lib);
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

/// `ail link --profile <name> [--output <path>] [--runtime-lib <path>]`
///
/// Resolves the latest persisted native artifact for `profile`, then links it
/// into an executable via the injectable `linker` boundary.
///
/// Default output path: `./<profile>` in the current working directory (e.g.
/// `./dev`).  Pass `--output` to override.
///
/// `runtime_lib` — when `Some`, the archive is appended to the linker arguments so
/// the native object's unresolved imports (`host_call`, `__ail_malloc`,
/// `ail_runtime_call`) are resolved at link time and the result is a
/// self-contained executable.  When `None`, a hint is printed in the output
/// reminding the user to supply the runtime library if the linker reports
/// undefined-symbol errors.
///
/// Errors (all `CliError::Domain`):
/// - No native artifact persisted for `profile` — suggests running `ail compile`.
/// - System linker unavailable or exited non-zero.
pub(crate) fn cmd_link(
    mode: OutputMode,
    profile: &str,
    output: Option<&Path>,
    runtime_lib: Option<&Path>,
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
        .link(object_path, &output_path, runtime_lib)
        .inspect_err(|e| {
            if mode == OutputMode::Json {
                print_error_response(json!({
                    "error": "linker_failed",
                    "profile": profile,
                    "object_path": object_path.to_string_lossy(),
                    "runtime_lib": runtime_lib.map(|p| p.to_string_lossy().into_owned()),
                    "message": e.to_string(),
                }));
            }
        })?;

    // 4. Emit result; include runtime-library hint when none was supplied.
    let runtime_lib_str = runtime_lib.map(|p| p.to_string_lossy().into_owned());
    let runtime_lib_hint = if runtime_lib.is_none() {
        "\nhint: supply --runtime-lib <path/to/ail_runtime.a> to resolve \
         host_call/__ail_malloc/ail_runtime_call at link time"
    } else {
        ""
    };
    let human_msg = format!(
        "profile: {profile}\nobject: {}\noutput: {}\nlinker_command: {}\nstatus: linked{runtime_lib_hint}",
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
            "runtime_lib": runtime_lib_str,
            "status": "linked",
        }),
    );
    Ok(())
}
