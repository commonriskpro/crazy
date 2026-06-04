// Tests for the WASM emission stage.
// Declared from wasm.rs as: #[cfg(test)] #[path = "wasm_tests.rs"] mod tests;

#[path = "wasm_tests/abi.rs"]
mod abi;
#[path = "wasm_tests/compound.rs"]
mod compound;
#[path = "wasm_tests/control_flow.rs"]
mod control_flow;
#[path = "wasm_tests/effects.rs"]
mod effects;
#[path = "wasm_tests/hash.rs"]
mod hash;
#[path = "wasm_tests/helpers.rs"]
mod helpers;
#[path = "wasm_tests/lambda_fold/mod.rs"]
mod lambda_fold;
#[path = "wasm_tests/sections.rs"]
mod sections;
