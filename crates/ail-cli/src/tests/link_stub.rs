// ── ail-cli::tests::link_stub ─────────────────────────────────────────────
//
// Tests for the runtime stub archive generator and the associated
// `ail link` sub-features:
//   --print-runtime-symbols
//   --emit-runtime-stub <path>
//
// All tests are pure-Rust and do not require a system linker, `ar`, or `cc`.
// They exercise:
//   S1 — RUNTIME_SYMBOLS constant is non-empty and contains the expected names.
//   S2 — build_runtime_stub_archive() returns bytes starting with `!<arch>\n`.
//   S3 — build_runtime_stub_archive() is deterministic.
//   S4 — cmd_emit_runtime_stub writes a valid archive to a temp file.
//   S5 — cmd_print_runtime_symbols succeeds (no panic/error).
//   S6 — build_emit_stub_result_json contract fields are present.
//   S7 — validate_link_mode_flags rejects both standalone flags at once.

use std::path::PathBuf;

use ail_compiler::RUNTIME_SYMBOLS;
use ail_compiler::build_runtime_stub_archive;

use crate::link_commands::{
    build_emit_stub_result_json, cmd_emit_runtime_stub, cmd_print_runtime_symbols,
    validate_link_mode_flags,
};
use crate::output::OutputMode;

// ── S1 — RUNTIME_SYMBOLS ─────────────────────────────────────────────────

// Scenario: RUNTIME_SYMBOLS contains exactly the three expected symbol names.
//   GIVEN RUNTIME_SYMBOLS constant
//   WHEN inspected
//   THEN it contains host_call, __ail_malloc, and ail_runtime_call
#[test]
fn runtime_symbols_contains_expected_names() {
    let syms: Vec<&str> = RUNTIME_SYMBOLS.to_vec();
    assert!(
        syms.contains(&"host_call"),
        "RUNTIME_SYMBOLS must contain 'host_call'; got: {syms:?}"
    );
    assert!(
        syms.contains(&"__ail_malloc"),
        "RUNTIME_SYMBOLS must contain '__ail_malloc'; got: {syms:?}"
    );
    assert!(
        syms.contains(&"ail_runtime_call"),
        "RUNTIME_SYMBOLS must contain 'ail_runtime_call'; got: {syms:?}"
    );
}

// Scenario: RUNTIME_SYMBOLS has exactly three entries.
#[test]
fn runtime_symbols_has_exactly_three_entries() {
    assert_eq!(
        RUNTIME_SYMBOLS.len(),
        3,
        "RUNTIME_SYMBOLS must have exactly 3 entries; got: {:?}",
        RUNTIME_SYMBOLS
    );
}

// ── S2 — ar magic bytes ───────────────────────────────────────────────────

// Scenario: build_runtime_stub_archive starts with `!<arch>\n`.
//   GIVEN build_runtime_stub_archive is called
//   WHEN the result is inspected
//   THEN the first 8 bytes are the BSD/GNU ar global header `!<arch>\n`
#[test]
fn stub_archive_starts_with_ar_magic() {
    let archive = build_runtime_stub_archive().expect("build_runtime_stub_archive must succeed");
    assert!(
        archive.len() >= 8,
        "stub archive must be at least 8 bytes; got {} bytes",
        archive.len()
    );
    assert_eq!(
        &archive[..8],
        b"!<arch>\n",
        "stub archive must start with `!<arch>\\n` (BSD/GNU ar magic)"
    );
}

// Scenario: stub archive is non-trivially long (contains object data).
//   GIVEN build_runtime_stub_archive is called
//   THEN the result is larger than the ar header + member header (> 68 bytes)
#[test]
fn stub_archive_has_non_trivial_size() {
    let archive = build_runtime_stub_archive().expect("build_runtime_stub_archive must succeed");
    // 8 (global) + 60 (member header) + at least 1 byte of object data
    assert!(
        archive.len() > 68,
        "stub archive must be longer than the bare header (68 bytes); got {} bytes",
        archive.len()
    );
}

// ── S3 — determinism ──────────────────────────────────────────────────────

// Scenario: build_runtime_stub_archive is deterministic.
//   GIVEN two calls to build_runtime_stub_archive with no other changes
//   WHEN the results are compared
//   THEN they are byte-identical
#[test]
fn stub_archive_is_deterministic() {
    let a1 = build_runtime_stub_archive().expect("first build must succeed");
    let a2 = build_runtime_stub_archive().expect("second build must succeed");
    assert_eq!(
        a1, a2,
        "build_runtime_stub_archive must produce byte-identical output on each call"
    );
}

// ── S4 — cmd_emit_runtime_stub writes to disk ─────────────────────────────

// Scenario: cmd_emit_runtime_stub writes a file with the ar magic header.
//   GIVEN a temp directory
//   WHEN cmd_emit_runtime_stub is called with a path inside it
//   THEN Ok is returned and the file exists with the correct ar magic
#[test]
fn cmd_emit_runtime_stub_writes_valid_archive() {
    let temp = tempfile::tempdir().expect("tempdir");
    let out_path = temp.path().join("ail_runtime.a");

    let result = cmd_emit_runtime_stub(OutputMode::Human, &out_path);
    assert!(
        result.is_ok(),
        "cmd_emit_runtime_stub must succeed; got: {result:?}"
    );

    assert!(
        out_path.exists(),
        "cmd_emit_runtime_stub must create the output file"
    );

    let bytes = std::fs::read(&out_path).expect("output file must be readable");
    assert!(
        bytes.len() >= 8,
        "output file must be at least 8 bytes; got {} bytes",
        bytes.len()
    );
    assert_eq!(
        &bytes[..8],
        b"!<arch>\n",
        "output file must start with the ar magic header"
    );
}

// Scenario: cmd_emit_runtime_stub in Json mode succeeds.
//   GIVEN a temp path
//   WHEN cmd_emit_runtime_stub is called with OutputMode::Json
//   THEN Ok is returned
#[test]
fn cmd_emit_runtime_stub_json_mode_succeeds() {
    let temp = tempfile::tempdir().expect("tempdir");
    let out_path = temp.path().join("ail_runtime_json.a");
    let result = cmd_emit_runtime_stub(OutputMode::Json, &out_path);
    assert!(
        result.is_ok(),
        "cmd_emit_runtime_stub in Json mode must succeed; got: {result:?}"
    );
}

// ── S5 — cmd_print_runtime_symbols ───────────────────────────────────────

// Scenario: cmd_print_runtime_symbols completes without panic.
//   GIVEN OutputMode::Human
//   WHEN cmd_print_runtime_symbols is called
//   THEN it returns without panicking
#[test]
fn cmd_print_runtime_symbols_human_mode_no_panic() {
    // Should not panic; no return value to assert on.
    cmd_print_runtime_symbols(OutputMode::Human);
}

// Scenario: cmd_print_runtime_symbols in Json mode completes without panic.
#[test]
fn cmd_print_runtime_symbols_json_mode_no_panic() {
    cmd_print_runtime_symbols(OutputMode::Json);
}

// ── S6 — build_emit_stub_result_json contract ────────────────────────────

// Scenario: build_emit_stub_result_json includes all required contract fields.
//   GIVEN a representative output path and size
//   WHEN build_emit_stub_result_json is called
//   THEN the result contains output_path, size_bytes, symbols, and status fields
#[test]
fn emit_stub_result_json_contract_fields_are_present() {
    let path = PathBuf::from("/tmp/ail_runtime.a");
    let value = build_emit_stub_result_json(&path, 1234);
    let obj = value.as_object().expect("result must be a JSON object");

    for key in &["output_path", "size_bytes", "symbols", "status"] {
        assert!(
            obj.contains_key(*key),
            "JSON contract must include key '{key}'; got keys: {:?}",
            obj.keys().collect::<Vec<_>>()
        );
    }

    assert_eq!(
        value["status"].as_str(),
        Some("emitted"),
        "status field must equal \"emitted\""
    );

    let syms = value["symbols"]
        .as_array()
        .expect("symbols must be an array");
    assert_eq!(
        syms.len(),
        3,
        "symbols array must have 3 entries; got: {syms:?}"
    );
}

// Scenario: build_emit_stub_result_json size_bytes matches the supplied value.
#[test]
fn emit_stub_result_json_size_bytes_matches_input() {
    let path = PathBuf::from("/tmp/stub.a");
    let value = build_emit_stub_result_json(&path, 42_000);
    assert_eq!(
        value["size_bytes"].as_u64(),
        Some(42_000),
        "size_bytes must equal the supplied byte count"
    );
}

// ── S7 — validate_link_mode_flags ────────────────────────────────────────

// Scenario: validate_link_mode_flags rejects both standalone flags supplied together.
//   --print-runtime-symbols and --emit-runtime-stub are standalone modes;
//   combining them is ambiguous and must produce an explicit error instead of
//   silently applying flag precedence.
//
//   GIVEN both print_runtime_symbols=true and emit_runtime_stub=true
//   WHEN validate_link_mode_flags is called
//   THEN it returns Err(CliError::Domain)
#[test]
fn validate_link_mode_flags_errors_when_both_set() {
    let result = validate_link_mode_flags(true, true);
    assert!(
        result.is_err(),
        "validate_link_mode_flags must return Err when both standalone flags are set"
    );
}

// Scenario: validate_link_mode_flags accepts each standalone flag alone.
//   GIVEN only one of the two standalone flags is set (or neither)
//   WHEN validate_link_mode_flags is called
//   THEN it returns Ok
#[test]
fn validate_link_mode_flags_accepts_single_or_neither() {
    assert!(
        validate_link_mode_flags(true, false).is_ok(),
        "print-only must be accepted"
    );
    assert!(
        validate_link_mode_flags(false, true).is_ok(),
        "emit-only must be accepted"
    );
    assert!(
        validate_link_mode_flags(false, false).is_ok(),
        "neither flag must be accepted (normal link mode)"
    );
}
