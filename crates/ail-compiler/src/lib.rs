//! # ail-compiler
//!
//! Pure deterministic transformation pipeline:
//! `SemanticGraph + VerificationReport → CoreIr → AnfIr → WasmArtifact | NativeArtifact`.
//!
//! # What this crate does
//! - Lowers a verified `SemanticGraph` through three IR stages (Core → ANF → WASM/native).
//! - Maintains a BLAKE3 hash chain across every stage for reproducibility.
//! - Emits structurally valid WASM via `wasm-encoder`; function bodies are
//!   `unreachable` stubs until Phase 8 adds expression lowering.
//! - Emits platform-native object files via Cranelift (Phase 17); function
//!   bodies are `trap` stubs until Phase 8+ adds expression lowering.
//!
//! # What this crate does NOT do
//! - No parsing, no source mutation, no runtime/Wasmtime dependency.
//! - Optimisation passes in `optimize.rs` (`optimize_bindings`,
//!   `eliminate_dead_pure`, `inline_small_pure`, `cse_bindings`) are not
//!   applied automatically — callers opt in explicitly.
//! - No expression / body codegen (deferred to Phase 8).

pub mod anf;
pub mod artifact_manifest;
pub mod cache;
pub mod compiler_report;
pub mod core_ir;
pub mod error;
pub mod expr_parser;
pub mod hash;
pub mod incremental;
pub mod lower;
pub mod native;
pub mod optimize;
pub mod wasm;

#[cfg(test)]
mod spike_v03;

pub use anf::{
    ANF_SCHEMA_VERSION, AnfBinding, AnfExpr, AnfIr, AnfMatchArm, AnfSelectClause, SourceMap,
    SourceMapEntry,
};
pub use artifact_manifest::ArtifactManifest;
pub use cache::{ArtifactCache, ArtifactEntry, MemoryArtifactCache};
pub use core_ir::{
    CoreExpr, CoreIr, CoreNode, CoreNodeKind, CoreType, LiteralValue, MatchArm, SelectClause,
    StageHashes,
};
pub use compiler_report::{CompilerReport, CompilerWarning, StageRecord};
pub use error::CompileError;
pub use expr_parser::{ParseError, parse_expr};
pub use incremental::{DirtySet, NodeHashes, compile_incremental, compute_node_hashes};
pub use lower::{
    is_report_accepted, lower_core_expr_to_anf, lower_to_anf, lower_to_anf_with_graph,
    lower_to_core_ir, nominal_to_core_type,
};
pub use native::{
    CapabilitiesManifest, CapabilityEntry, NativeArtifact, emit_native, emit_native_with_profile,
};
pub use optimize::optimize_bindings;
pub use wasm::{WasmArtifact, emit_wasm, emit_wasm_with_profile};
