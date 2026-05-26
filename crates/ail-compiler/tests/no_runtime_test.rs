// ── ail-compiler::no_runtime_test ────────────────────────────────────────
//
// Task 3.6: Guard against accidental runtime dependency introduction.
//
// Spec: `cargo tree -p ail-compiler` must NOT contain public `wasmtime` or
// `wasmer` runtime crates.
//
// This test executes `cargo tree` as a subprocess and asserts the output
// contains neither runtime. It is an integration guard — if someone adds the
// public wasmtime or wasmer runtime crate to Cargo.toml, this test will fail
// loudly.
//
// Cranelift 0.132 can pull `wasmtime-internal-*` support crates from the shared
// Wasmtime/Cranelift codebase. Those are allowed because they are not the
// public Wasmtime runtime crate; ail-runtime remains the runtime owner.

use std::process::Command;

// ── no-runtime guard ─────────────────────────────────────────────────────

fn cargo_tree_contains_public_crate(stdout: &str, crate_name: &str) -> bool {
    let marker = format!("{crate_name} v");

    stdout.lines().any(|line| line.contains(&marker))
}

// Spec: ail-compiler dependency tree must NOT include public wasmtime.
// The guard runs `cargo tree -p ail-compiler` and checks for the crate name.
#[test]
fn wasmtime_is_not_in_dependency_tree() {
    let output = Command::new("cargo")
        .args(["tree", "-p", "ail-compiler"])
        .output()
        .expect("failed to run `cargo tree`");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let has_wasmtime = cargo_tree_contains_public_crate(&stdout, "wasmtime");

    assert!(
        !has_wasmtime,
        "ail-compiler must NOT depend on public wasmtime:\n{}",
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
    let has_wasmer = cargo_tree_contains_public_crate(&stdout, "wasmer");

    assert!(
        !has_wasmer,
        "ail-compiler must NOT depend on public wasmer:\n{}",
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
