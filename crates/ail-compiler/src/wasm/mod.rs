// ── ail-compiler::wasm ────────────────────────────────────────────────────
//
// WASM emission — the third and final pipeline stage.
//
// # Pre-condition
//
// `emit_wasm` MUST be called with an `AnfIr` produced by `lower_to_anf`.
// The `anf_ir_hash` field in `stage_hashes` must be `Some(...)`.
// If it is `None`, `Err(CompileError::EncodingError)` is returned.
//
// # Module layout
//
// - `lambdas` — Lambda/Fold discovery and pre-flight gates.
// - `emit`    — WASM module assembly and artifact sidecar sealing.

mod emit;
mod lambdas;

pub use crate::wasm_abi::{
    ABI_VERSION, AbiDescriptor, WasmScalarType, WasmTypeDescriptor, derive_wasm_type,
};
pub use crate::wasm_artifact::WasmArtifact;
pub use emit::{emit_wasm, emit_wasm_with_profile};

#[cfg(test)]
#[path = "../wasm_tests.rs"]
mod tests;
