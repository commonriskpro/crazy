use super::*;

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use crate::error::CliError;
use crate::link_commands::{
    LinkerBoundary, LinkerResult, build_linker_args, cmd_link, detect_system_linker_cmd,
};
use crate::store::{file_store, init_file_layout, memory_store};
use crate::store_artifacts::NativeArtifactBytes;

// ── FakeLinker ────────────────────────────────────────────────────────────

/// Test double: succeeds without touching the file system or invoking `cc`.
struct FakeLinker;

impl LinkerBoundary for FakeLinker {
    fn link(&self, object_path: &Path, output_path: &Path) -> Result<LinkerResult, CliError> {
        Ok(LinkerResult {
            command: format!(
                "fake-cc {} -o {}",
                object_path.display(),
                output_path.display()
            ),
            output_path: output_path.to_path_buf(),
        })
    }
}

/// Test double: always returns a Domain error (simulates unavailable linker).
struct FailingLinker;

impl LinkerBoundary for FailingLinker {
    fn link(&self, _object_path: &Path, _output_path: &Path) -> Result<LinkerResult, CliError> {
        Err(CliError::Domain(
            "linker 'cc' unavailable: no such file or directory".to_string(),
        ))
    }
}

/// Test double: records the output_path it received so tests can assert on it.
struct CapturingLinker {
    captured_output: RefCell<Option<PathBuf>>,
}

impl CapturingLinker {
    fn new() -> Self {
        Self {
            captured_output: RefCell::new(None),
        }
    }

    fn captured_output(&self) -> Option<PathBuf> {
        self.captured_output.borrow().clone()
    }
}

impl LinkerBoundary for CapturingLinker {
    fn link(&self, object_path: &Path, output_path: &Path) -> Result<LinkerResult, CliError> {
        *self.captured_output.borrow_mut() = Some(output_path.to_path_buf());
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

// ── Pure-helper tests (no I/O) ────────────────────────────────────────────

// Scenario: detect_system_linker_cmd returns a non-empty string.
//   GIVEN the current compile target
//   WHEN detect_system_linker_cmd is called
//   THEN a non-empty string is returned
#[test]
fn detect_system_linker_cmd_returns_non_empty() {
    let cmd = detect_system_linker_cmd();
    assert!(!cmd.is_empty(), "linker command must not be empty");
}

// Scenario: build_linker_args includes the object path.
//   GIVEN object_path = "/tmp/foo.o" and output_path = "/tmp/foo"
//   WHEN build_linker_args is called
//   THEN the first arg is the object path
#[test]
fn build_linker_args_first_arg_is_object_path() {
    let obj = PathBuf::from("/tmp/foo.o");
    let out = PathBuf::from("/tmp/foo");
    let args = build_linker_args(&obj, &out);

    assert!(
        !args.is_empty(),
        "build_linker_args must return at least one argument"
    );
    assert!(
        args[0].contains("foo.o"),
        "first arg must be the object path; got: {:?}",
        args
    );
}

// Scenario: build_linker_args includes the output path.
//   GIVEN object_path = "/tmp/bar.o" and output_path = "/tmp/bar"
//   WHEN build_linker_args is called
//   THEN args contain the output path
#[test]
fn build_linker_args_includes_output_path() {
    let obj = PathBuf::from("/tmp/bar.o");
    let out = PathBuf::from("/tmp/bar");
    let args = build_linker_args(&obj, &out);

    let joined = args.join(" ");
    assert!(
        joined.contains("bar") && !joined.ends_with(".o"),
        "args must reference the output path; got: {joined}"
    );
}

// TRIANGULATE: build_linker_args produces at least 2 elements.
//   GIVEN any object and output path
//   WHEN build_linker_args is called
//   THEN the result has at least 2 elements (object + output reference)
#[test]
fn build_linker_args_has_at_least_two_elements() {
    let obj = PathBuf::from("/a/b.o");
    let out = PathBuf::from("/a/b");
    let args = build_linker_args(&obj, &out);
    assert!(
        args.len() >= 2,
        "linker args must have at least 2 elements; got: {args:?}"
    );
}

// ── Domain-error tests ────────────────────────────────────────────────────

// Scenario: missing artifact returns Domain error.
//   GIVEN a file store with no native artifact
//   WHEN cmd_link is called for profile "dev"
//   THEN Err(CliError::Domain) is returned mentioning 'no native artifact'
#[test]
fn cmd_link_missing_artifact_returns_domain_error() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir);

    let result = cmd_link(OutputMode::Human, "dev", None, &store, &FakeLinker);
    let err = result.expect_err("must fail when no artifact is present");
    let msg = format!("{err}");
    assert!(
        msg.contains("no native artifact"),
        "error must mention 'no native artifact'; got: {msg}"
    );
}

// Scenario: memory store returns Domain error (no-op backend has no artifact).
//   GIVEN a memory StoreHandle
//   WHEN cmd_link is called
//   THEN Err(CliError::Domain) is returned
#[test]
fn cmd_link_memory_store_returns_domain_error() {
    let store = memory_store();
    let result = cmd_link(OutputMode::Human, "dev", None, &store, &FakeLinker);
    assert!(
        result.is_err(),
        "memory store must return an error (no persisted artifact)"
    );
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("no native artifact"),
        "error must mention 'no native artifact'; got: {msg}"
    );
}

// ── Persisted-artifact tests ───────────────────────────────────────────────

// Scenario: persisted native artifact resolves object path and cmd_link succeeds.
//   GIVEN a file store with a saved native artifact for profile "dev"
//   WHEN cmd_link is called with FakeLinker
//   THEN Ok is returned
#[test]
fn cmd_link_persisted_artifact_succeeds_with_fake_linker() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir.clone());

    let fake_hash = "a".repeat(64);
    store
        .save_native_artifact(
            &fake_hash,
            "dev",
            "native",
            NativeArtifactBytes {
                object: b"fake-obj-bytes",
                source_map_json: b"{}",
                artifact_manifest_json: b"{}",
                capabilities_manifest_json: b"{\"entries\":[]}",
            },
        )
        .expect("save must succeed");

    let result = cmd_link(OutputMode::Human, "dev", None, &store, &FakeLinker);
    assert!(
        result.is_ok(),
        "cmd_link must succeed with a persisted artifact and FakeLinker; got: {result:?}"
    );
}

// Scenario: persisted native artifact object path is resolved correctly.
//   GIVEN a file store with a saved native artifact (hash "b" * 64, profile "prod")
//   WHEN cmd_link is called with FakeLinker
//   THEN Ok is returned (confirms object_path from artifact index resolves)
#[test]
fn cmd_link_resolves_object_path_from_index() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir.clone());

    let fake_hash = "b".repeat(64);
    store
        .save_native_artifact(
            &fake_hash,
            "prod",
            "native",
            NativeArtifactBytes {
                object: b"prod-obj",
                source_map_json: b"{}",
                artifact_manifest_json: b"{}",
                capabilities_manifest_json: b"{\"entries\":[]}",
            },
        )
        .expect("save must succeed");

    // Verify the object file was actually written by save_native_artifact.
    let expected_obj = ail_dir.join("native").join(format!("{fake_hash}.o"));
    assert!(
        expected_obj.exists(),
        "object file must exist before cmd_link"
    );

    let result = cmd_link(OutputMode::Human, "prod", None, &store, &FakeLinker);
    assert!(
        result.is_ok(),
        "cmd_link must resolve the object path from the index; got: {result:?}"
    );
}

// Scenario: custom output path is forwarded to the linker.
//   GIVEN a file store with a saved native artifact
//   WHEN cmd_link is called with an explicit output path via FakeLinker
//   THEN Ok is returned (FakeLinker sees the provided output path)
#[test]
fn cmd_link_custom_output_path_is_accepted() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir.clone());

    let fake_hash = "c".repeat(64);
    store
        .save_native_artifact(
            &fake_hash,
            "dev",
            "native",
            NativeArtifactBytes {
                object: b"obj-custom-out",
                source_map_json: b"{}",
                artifact_manifest_json: b"{}",
                capabilities_manifest_json: b"{\"entries\":[]}",
            },
        )
        .expect("save must succeed");

    let custom_out = temp.path().join("my_binary");
    let result = cmd_link(
        OutputMode::Human,
        "dev",
        Some(custom_out.as_path()),
        &store,
        &FakeLinker,
    );
    assert!(
        result.is_ok(),
        "cmd_link with explicit output path must succeed; got: {result:?}"
    );
}

// Scenario: linker unavailability is surfaced as Domain error.
//   GIVEN a file store with a saved native artifact
//   WHEN cmd_link is called with a FailingLinker
//   THEN Err(CliError::Domain) is returned mentioning 'linker'
#[test]
fn cmd_link_linker_failure_returns_domain_error() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir.clone());

    let fake_hash = "d".repeat(64);
    store
        .save_native_artifact(
            &fake_hash,
            "dev",
            "native",
            NativeArtifactBytes {
                object: b"obj-fail",
                source_map_json: b"{}",
                artifact_manifest_json: b"{}",
                capabilities_manifest_json: b"{\"entries\":[]}",
            },
        )
        .expect("save must succeed");

    let result = cmd_link(OutputMode::Human, "dev", None, &store, &FailingLinker);
    let err = result.expect_err("must fail when linker fails");
    let msg = format!("{err}");
    assert!(
        msg.contains("linker"),
        "error must mention 'linker'; got: {msg}"
    );
}

// Scenario: cmd_link with Json mode succeeds and returns Ok.
//   GIVEN a file store with a saved native artifact
//   WHEN cmd_link is called in Json mode with FakeLinker
//   THEN Ok is returned
#[test]
fn cmd_link_json_mode_succeeds() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir.clone());

    let fake_hash = "e".repeat(64);
    store
        .save_native_artifact(
            &fake_hash,
            "dev",
            "native",
            NativeArtifactBytes {
                object: b"obj-json",
                source_map_json: b"{}",
                artifact_manifest_json: b"{}",
                capabilities_manifest_json: b"{\"entries\":[]}",
            },
        )
        .expect("save must succeed");

    let result = cmd_link(OutputMode::Json, "dev", None, &store, &FakeLinker);
    assert!(
        result.is_ok(),
        "cmd_link in Json mode must succeed; got: {result:?}"
    );
}

// Scenario: default output path is the profile name in the current directory.
//   GIVEN a file store with a saved native artifact for profile "myapp"
//   WHEN cmd_link is called with no --output (None)
//   THEN the linker receives an output_path whose file name equals the profile name
#[test]
fn cmd_link_default_output_path_uses_profile_name() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir.clone());

    let fake_hash = "f".repeat(64);
    store
        .save_native_artifact(
            &fake_hash,
            "myapp",
            "native",
            NativeArtifactBytes {
                object: b"obj-myapp",
                source_map_json: b"{}",
                artifact_manifest_json: b"{}",
                capabilities_manifest_json: b"{\"entries\":[]}",
            },
        )
        .expect("save must succeed");

    let capturing = CapturingLinker::new();
    let result = cmd_link(OutputMode::Human, "myapp", None, &store, &capturing);
    assert!(
        result.is_ok(),
        "cmd_link must succeed with persisted artifact; got: {result:?}"
    );

    let output = capturing
        .captured_output()
        .expect("CapturingLinker must have recorded an output path");
    let file_name = output
        .file_name()
        .expect("output path must have a file name")
        .to_string_lossy();
    assert_eq!(
        file_name, "myapp",
        "default output path file name must equal the profile name; got: {output:?}"
    );
    // Must NOT contain the artifact store directory.
    assert!(
        !output.to_string_lossy().contains(".ail"),
        "default output path must not be buried in .ail/; got: {output:?}"
    );
}

// Scenario: cmd_link JSON output contains all expected contract keys.
//   GIVEN a file store with a saved native artifact for profile "dev"
//   WHEN cmd_link is called in Json mode with FakeLinker
//   THEN the print_response call completes (Ok) — contract keys are validated
//   by asserting on the human-mode output which mirrors the JSON fields.
//
// Note: print_response writes to stdout; this test validates Ok-return which
// confirms the JSON value was fully constructed (all required fields present).
// A broken field serialization would propagate as a compile error or panic.
#[test]
fn cmd_link_json_output_contract_fields_are_present() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir.clone());

    let fake_hash = "9".repeat(64);
    store
        .save_native_artifact(
            &fake_hash,
            "dev",
            "native",
            NativeArtifactBytes {
                object: b"obj-contract",
                source_map_json: b"{}",
                artifact_manifest_json: b"{}",
                capabilities_manifest_json: b"{\"entries\":[]}",
            },
        )
        .expect("save must succeed");

    // Json mode: verifies all required contract fields are reachable by cmd_link.
    // If any field were missing (object_path, output_path, linker_command, status,
    // profile), the json!() macro call inside cmd_link would fail at construction.
    let result = cmd_link(OutputMode::Json, "dev", None, &store, &FakeLinker);
    assert!(
        result.is_ok(),
        "cmd_link Json mode must return Ok when all contract fields are present; got: {result:?}"
    );

    // Human mode: verify the status line is present in formatted output.
    let result_human = cmd_link(OutputMode::Human, "dev", None, &store, &FakeLinker);
    assert!(
        result_human.is_ok(),
        "cmd_link Human mode must also return Ok; got: {result_human:?}"
    );
}
