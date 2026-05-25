// ── ail-cli::tests::link_ensure ───────────────────────────────────────────
//
// Tests for `ail link --ensure-runtime-stub`:
//   ensure_runtime_stub_at() — idempotent archive creation in a project dir
//   validate_link_mode_flags() — new conflict checks for ensure flag
//
// All tests are pure-Rust (no system linker, ar, or cc required).
// They exercise:
//   E1 — ensure_runtime_stub_at creates the archive when it does not exist.
//   E2 — ensure_runtime_stub_at is idempotent (second call is a no-op).
//   E3 — ensure_runtime_stub_at returns a path to an existing file unchanged.
//   E4 — ensure_runtime_stub_at creates the parent directory when absent.
//   E5 — validate_link_mode_flags rejects ensure + emit together.
//   E6 — validate_link_mode_flags rejects ensure + print together.
//   E7 — validate_link_mode_flags rejects ensure + runtime_lib together.
//   E8 — validate_link_mode_flags accepts ensure alone.
//   E9 — cmd_link with a path from ensure_runtime_stub_at forwards it to linker.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use crate::error::CliError;
use crate::link_commands::{
    LinkerBoundary, LinkerResult, RUNTIME_STUB_SUBDIR, cmd_link, ensure_runtime_stub_at,
    validate_link_mode_flags,
};
use crate::output::OutputMode;
use crate::store::{file_store, init_file_layout};
use crate::store_artifacts::NativeArtifactBytes;

// ── CapturingLinker (local copy) ─────────────────────────────────────────

/// Records the runtime_lib path received by the linker boundary.
struct CapturingLinker {
    captured_runtime_lib: RefCell<Option<PathBuf>>,
}

impl CapturingLinker {
    fn new() -> Self {
        Self {
            captured_runtime_lib: RefCell::new(None),
        }
    }

    fn captured_runtime_lib(&self) -> Option<PathBuf> {
        self.captured_runtime_lib.borrow().clone()
    }
}

impl LinkerBoundary for CapturingLinker {
    fn link(
        &self,
        object_path: &Path,
        output_path: &Path,
        runtime_lib: Option<&Path>,
    ) -> Result<LinkerResult, CliError> {
        *self.captured_runtime_lib.borrow_mut() = runtime_lib.map(|p| p.to_path_buf());
        Ok(LinkerResult {
            command: format!(
                "capturing-cc {} -o {}",
                object_path.display(),
                output_path.display()
            ),
            output_path: output_path.to_path_buf(),
        })
    }
}

// ── E1 — creates archive when missing ────────────────────────────────────

// Scenario: ensure_runtime_stub_at creates ail_runtime.a when the file is absent.
//   GIVEN a temp directory with no existing ail_runtime.a
//   WHEN ensure_runtime_stub_at is called
//   THEN Ok is returned, the file exists, and starts with the ar magic header
#[test]
fn ensure_runtime_stub_at_creates_archive_when_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let stub_dir = temp.path().join(RUNTIME_STUB_SUBDIR);

    let result = ensure_runtime_stub_at(&stub_dir);
    assert!(
        result.is_ok(),
        "ensure_runtime_stub_at must succeed; got: {result:?}"
    );

    let stub_path = result.unwrap();
    assert!(
        stub_path.exists(),
        "ail_runtime.a must be created at {stub_path:?}"
    );

    let bytes = std::fs::read(&stub_path).expect("must be readable");
    assert!(
        bytes.len() >= 8,
        "stub archive must be at least 8 bytes; got {} bytes",
        bytes.len()
    );
    assert_eq!(
        &bytes[..8],
        b"!<arch>\n",
        "stub archive must start with ar magic `!<arch>\\n`"
    );
}

// ── E2 — idempotent: second call is a no-op ───────────────────────────────

// Scenario: calling ensure_runtime_stub_at twice does not overwrite or error.
//   GIVEN ensure_runtime_stub_at has already been called once
//   WHEN it is called a second time
//   THEN Ok is returned with the same path; byte contents are unchanged
#[test]
fn ensure_runtime_stub_at_is_idempotent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let stub_dir = temp.path().join(RUNTIME_STUB_SUBDIR);

    let path1 = ensure_runtime_stub_at(&stub_dir).expect("first call must succeed");
    let bytes_after_first = std::fs::read(&path1).expect("file must be readable");

    let path2 = ensure_runtime_stub_at(&stub_dir).expect("second call must succeed");
    let bytes_after_second = std::fs::read(&path2).expect("file must still be readable");

    assert_eq!(path1, path2, "both calls must return the same path");
    assert_eq!(
        bytes_after_first, bytes_after_second,
        "file contents must be byte-identical after two calls"
    );
}

// ── E3 — returns existing file path unchanged ─────────────────────────────

// Scenario: when the archive already exists, ensure returns its path without touching it.
//   GIVEN a pre-existing file at stub_dir/ail_runtime.a with sentinel bytes
//   WHEN ensure_runtime_stub_at is called
//   THEN Ok is returned, the file still contains the sentinel bytes (not regenerated)
#[test]
fn ensure_runtime_stub_at_returns_existing_file_unchanged() {
    let temp = tempfile::tempdir().expect("tempdir");
    let stub_dir = temp.path().join(RUNTIME_STUB_SUBDIR);
    std::fs::create_dir_all(&stub_dir).expect("create dir");

    let stub_path = stub_dir.join("ail_runtime.a");
    let sentinel = b"SENTINEL_BYTES_NOT_A_REAL_ARCHIVE";
    std::fs::write(&stub_path, sentinel).expect("write sentinel");

    let result = ensure_runtime_stub_at(&stub_dir).expect("must succeed");
    assert_eq!(
        result, stub_path,
        "returned path must point to the existing file"
    );

    let contents = std::fs::read(&stub_path).expect("file must be readable");
    assert_eq!(
        contents, sentinel,
        "pre-existing file must not be overwritten; contents changed"
    );
}

// ── E4 — creates parent directory when absent ─────────────────────────────

// Scenario: ensure_runtime_stub_at creates stub_dir when it does not exist.
//   GIVEN a stub_dir that does not yet exist
//   WHEN ensure_runtime_stub_at is called
//   THEN Ok is returned and stub_dir was created by the function
#[test]
fn ensure_runtime_stub_at_creates_parent_directory() {
    let temp = tempfile::tempdir().expect("tempdir");
    // Use a nested path that does not exist yet.
    let stub_dir = temp.path().join("nested").join(RUNTIME_STUB_SUBDIR);
    assert!(
        !stub_dir.exists(),
        "stub_dir must not exist before the call; got: {stub_dir:?}"
    );

    let result = ensure_runtime_stub_at(&stub_dir);
    assert!(
        result.is_ok(),
        "ensure_runtime_stub_at must succeed even when dir is missing; got: {result:?}"
    );
    assert!(
        stub_dir.exists(),
        "stub_dir must be created by ensure_runtime_stub_at"
    );
    assert!(
        stub_dir.join("ail_runtime.a").exists(),
        "ail_runtime.a must exist inside the newly created dir"
    );
}

// ── E5 — validate rejects ensure + emit ───────────────────────────────────

// Scenario: --ensure-runtime-stub and --emit-runtime-stub cannot be combined.
//   GIVEN ensure_runtime_stub=true, emit_runtime_stub=true
//   WHEN validate_link_mode_flags is called
//   THEN Err(CliError::Domain) is returned
#[test]
fn validate_link_mode_flags_rejects_ensure_and_emit() {
    let result = validate_link_mode_flags(false, true, true, false);
    assert!(result.is_err(), "ensure + emit must be rejected; got Ok");
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("--ensure-runtime-stub") || msg.contains("ensure"),
        "error must mention ensure flag; got: {msg}"
    );
}

// ── E6 — validate rejects ensure + print ─────────────────────────────────

// Scenario: --ensure-runtime-stub and --print-runtime-symbols cannot be combined.
//   GIVEN print_runtime_symbols=true, ensure_runtime_stub=true
//   WHEN validate_link_mode_flags is called
//   THEN Err(CliError::Domain) is returned
#[test]
fn validate_link_mode_flags_rejects_ensure_and_print() {
    let result = validate_link_mode_flags(true, false, true, false);
    assert!(result.is_err(), "ensure + print must be rejected; got Ok");
}

// ── E7 — validate rejects ensure + runtime_lib ───────────────────────────

// Scenario: --ensure-runtime-stub and --runtime-lib cannot be combined.
//   GIVEN ensure_runtime_stub=true, has_runtime_lib=true
//   WHEN validate_link_mode_flags is called
//   THEN Err(CliError::Domain) is returned
#[test]
fn validate_link_mode_flags_rejects_ensure_and_runtime_lib() {
    let result = validate_link_mode_flags(false, false, true, true);
    assert!(
        result.is_err(),
        "ensure + runtime_lib must be rejected; got Ok"
    );
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("--runtime-lib") || msg.contains("runtime-lib"),
        "error must mention --runtime-lib; got: {msg}"
    );
}

// ── E8 — validate accepts ensure alone ───────────────────────────────────

// Scenario: --ensure-runtime-stub alone is valid.
//   GIVEN ensure_runtime_stub=true, all other flags false/false
//   WHEN validate_link_mode_flags is called
//   THEN Ok is returned
#[test]
fn validate_link_mode_flags_accepts_ensure_alone() {
    let result = validate_link_mode_flags(false, false, true, false);
    assert!(
        result.is_ok(),
        "ensure alone must be accepted; got: {result:?}"
    );
}

// ── E9 — cmd_link receives the ensured path ───────────────────────────────

// Scenario: when ensure_runtime_stub_at succeeds, cmd_link receives the path.
//   GIVEN a file store with a persisted native artifact
//   AND ensure_runtime_stub_at returns a valid path
//   WHEN cmd_link is called with that path
//   THEN CapturingLinker records the stub archive path
#[test]
fn cmd_link_with_ensured_stub_path_forwards_path_to_linker() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir.clone());

    // Persist a fake native artifact.
    let fake_hash = "e9".repeat(32); // 64-char hex
    store
        .save_native_artifact(
            &fake_hash,
            "dev",
            "native",
            NativeArtifactBytes {
                object: b"obj-ensure",
                source_map_json: b"{}",
                artifact_manifest_json: b"{}",
                capabilities_manifest_json: b"{\"entries\":[]}",
            },
        )
        .expect("save must succeed");

    // Build the ensured stub path using the canonical subdir.
    let stub_dir = temp.path().join(".ail").join(RUNTIME_STUB_SUBDIR);
    let ensured_path = ensure_runtime_stub_at(&stub_dir).expect("ensure must succeed");

    // Invoke cmd_link with the ensured path and capture what the linker sees.
    let capturing = CapturingLinker::new();
    let result = cmd_link(
        OutputMode::Human,
        "dev",
        None,
        Some(ensured_path.as_path()),
        &store,
        &capturing,
    );
    assert!(
        result.is_ok(),
        "cmd_link with ensured path must succeed; got: {result:?}"
    );

    let recorded_lib = capturing
        .captured_runtime_lib()
        .expect("CapturingLinker must have recorded a runtime_lib path");
    assert_eq!(
        recorded_lib, ensured_path,
        "runtime_lib forwarded to linker must equal the ensured path"
    );
    assert!(
        recorded_lib
            .file_name()
            .map(|n| n == "ail_runtime.a")
            .unwrap_or(false),
        "runtime_lib file name must be 'ail_runtime.a'; got: {recorded_lib:?}"
    );
}
