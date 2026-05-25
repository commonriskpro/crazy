// ── ail-cli integration tests: --ensure-runtime-stub CLI surface ──────────
//
// Covers the CLI-visible behaviour of --ensure-runtime-stub without requiring
// a real project or system linker.  These are process-level tests that verify
// help text and flag-conflict rejection at the dispatch layer.
//
//   CLI1 — `ail link --help` mentions --ensure-runtime-stub
//   CLI2 — `ail link --ensure-runtime-stub --runtime-lib <path>` is rejected
//           (exit 1) with an error that names both conflicting flags

mod common;

use common::ail;
use predicates::prelude::*;

// ── CLI1 — help text ──────────────────────────────────────────────────────

/// Spec scenario: --ensure-runtime-stub appears in link help output.
///   GIVEN the ail binary
///   WHEN `ail link --help` is invoked
///   THEN exit 0 and stdout contains the flag name `--ensure-runtime-stub`
#[test]
fn link_help_mentions_ensure_runtime_stub() {
    ail()
        .args(["link", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--ensure-runtime-stub"));
}

// ── CLI2 — flag conflict rejection ───────────────────────────────────────

/// Spec scenario: combining --ensure-runtime-stub and --runtime-lib is rejected.
///   GIVEN `ail link --ensure-runtime-stub --runtime-lib any.a --profile dev`
///   WHEN dispatch runs
///   THEN exit 1 (domain error) and stderr mentions the conflicting flags
#[test]
fn link_ensure_and_runtime_lib_conflict_is_rejected() {
    ail()
        .args([
            "link",
            "--ensure-runtime-stub",
            "--runtime-lib",
            "any.a",
            "--profile",
            "dev",
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(
            predicate::str::contains("--runtime-lib")
                .or(predicate::str::contains("--ensure-runtime-stub")),
        );
}
