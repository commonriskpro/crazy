// ── ail-compiler::no_runtime_test ────────────────────────────────────────
//
// Task 3.6: Guard against accidental runtime dependency introduction.
//
// Spec: `cargo tree -p ail-compiler` must NOT contain `wasmtime` or `wasmer`.
//
// This test executes `cargo tree` as a subprocess and asserts the output
// contains neither runtime. It is an integration guard — if someone adds
// wasmtime or wasmer to Cargo.toml, this test will fail loudly.

use std::process::Command;

// ── no-runtime guard ─────────────────────────────────────────────────────

// Spec: ail-compiler dependency tree must NOT include wasmtime.
// The guard runs `cargo tree -p ail-compiler` and checks for the crate name.
#[test]
fn wasmtime_is_not_in_dependency_tree() {
    let output = Command::new("cargo")
        .args(["tree", "-p", "ail-compiler"])
        .output()
        .expect("failed to run `cargo tree`");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let count = stdout.lines().filter(|l| l.contains("wasmtime")).count();

    assert_eq!(
        count, 0,
        "ail-compiler must NOT depend on wasmtime (found {count} occurrences in cargo tree):\n{}",
        stdout
    );
}

// TRIANGULATE: ail-compiler dependency tree must NOT include wasmer.
#[test]
fn wasmer_is_not_in_dependency_tree() {
    let output = Command::new("cargo")
        .args(["tree", "-p", "ail-compiler"])
        .output()
        .expect("failed to run `cargo tree`");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let count = stdout.lines().filter(|l| l.contains("wasmer")).count();

    assert_eq!(
        count, 0,
        "ail-compiler must NOT depend on wasmer (found {count} occurrences in cargo tree):\n{}",
        stdout
    );
}

// Sanity check: wasm-encoder IS present (confirms tree ran correctly).
#[test]
fn wasm_encoder_is_in_dependency_tree() {
    let output = Command::new("cargo")
        .args(["tree", "-p", "ail-compiler"])
        .output()
        .expect("failed to run `cargo tree`");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("wasm-encoder"),
        "cargo tree must show wasm-encoder as a dependency of ail-compiler"
    );
}
