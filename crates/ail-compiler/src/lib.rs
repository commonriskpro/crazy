//! # ail-compiler
//!
//! Pure deterministic transformation pipeline:
//! `SemanticGraph + VerificationReport → CoreIr → AnfIr → WasmArtifact`.
//!
//! # What this crate does
//! - Lowers a verified `SemanticGraph` through three IR stages (Core → ANF → WASM).
//! - Maintains a BLAKE3 hash chain across every stage for reproducibility.
//! - Emits structurally valid WASM via `wasm-encoder`; function bodies are
//!   `unreachable` stubs until Phase 8 adds expression lowering.
//!
//! # What this crate does NOT do
//! - No parsing, no source mutation, no runtime/Wasmtime dependency.
//! - No optimisation passes.
//! - No expression / body codegen (deferred to Phase 8).

pub mod anf;
pub mod core_ir;
pub mod error;
pub mod hash;
pub mod lower;
pub mod wasm;

#[cfg(test)]
mod spike_v03;

pub use anf::{AnfBinding, AnfIr};
pub use core_ir::{CoreIr, CoreNode, CoreNodeKind, StageHashes};
pub use error::CompileError;
pub use lower::{is_report_accepted, lower_to_anf, lower_to_core_ir};
pub use wasm::{WasmArtifact, emit_wasm};
