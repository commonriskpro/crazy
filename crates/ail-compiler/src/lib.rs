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
//! - No optimisation passes.
//! - No expression / body codegen (deferred to Phase 8).

pub mod anf;
pub mod artifact_manifest;
pub mod cache;
pub mod core_ir;
pub mod error;
pub mod hash;
pub mod incremental;
pub mod lower;
pub mod native;
pub mod wasm;

#[cfg(test)]
mod spike_v03;

pub use anf::{ANF_SCHEMA_VERSION, AnfBinding, AnfExpr, AnfIr, SourceMap, SourceMapEntry};
pub use anf::AnfMatchArm;
pub use artifact_manifest::ArtifactManifest;
pub use cache::{ArtifactCache, ArtifactEntry, MemoryArtifactCache};
pub use core_ir::{
    CoreExpr, CoreIr, CoreNode, CoreNodeKind, CoreType, LiteralValue, MatchArm, StageHashes,
};
pub use error::CompileError;
pub use incremental::{DirtySet, NodeHashes, compile_incremental, compute_node_hashes};
pub use lower::{
    is_report_accepted, lower_core_expr_to_anf, lower_to_anf, lower_to_anf_with_graph,
    lower_to_core_ir, nominal_to_core_type,
};
pub use native::{CapabilitiesManifest, CapabilityEntry, NativeArtifact, emit_native};
pub use wasm::{WasmArtifact, emit_wasm};
