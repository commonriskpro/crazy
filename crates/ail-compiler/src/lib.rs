//! # ail-compiler
//!
//! Pure deterministic transformation pipeline:
//! `SemanticGraph + VerificationReport → CoreIr → AnfIr → WasmArtifact | NativeArtifact`.
//!
//! # What this crate does
//! - Lowers a verified `SemanticGraph` through three IR stages (Core → ANF → WASM/native).
//! - Maintains a BLAKE3 hash chain across every stage for reproducibility.
//! - Emits structurally valid WASM via `wasm-encoder`; function bodies emit
//!   real IR for arithmetic, control-flow, EffectCall, and compound types
//!   (records/variants/lists/tuples).
//!   **WASM Lambda**: top-level Lambda bindings emit the body directly with
//!   captures and Lambda params as WASM function locals.  Nested Lambda
//!   sub-expressions emit a closure env in linear memory matching the native
//!   backend layout: `[fn_idx: i64, cap_count: i64, cap0: i64, ...]`; the
//!   `fn_idx` field is a placeholder (0) until a future element-section pass
//!   adds call_indirect support.  Concurrency expressions are stubs.
//! - Emits platform-native object files via Cranelift (Phase 17); Phase 8
//!   expression lowering covers arithmetic, control-flow, loops, match, text
//!   literals, records/variants/lists/tuples, EffectCall, and Lambda.
//!   Lambda with no captures returns a bare function pointer (I64).
//!   Lambda with captures heap-allocates a closure env struct carrying the
//!   function pointer and captured values by value — captures are not
//!   silently dropped.  Closure invocation / call-site ABI is deferred to
//!   Phase 9+.  Concurrency and resource ops dispatch via imported
//!   `ail_runtime_call`; the runtime implementation is deferred to Phase 9+.
//!
//! # What this crate does NOT do
//! - No parsing, no source mutation, no runtime/Wasmtime dependency.
//! - Optimisation passes in `optimize.rs` (`optimize_bindings`,
//!   `eliminate_dead_pure`, `inline_small_pure`, `cse_bindings`) are not
//!   applied automatically — callers opt in explicitly.
//! - No closure invocation through the closure ABI; no concurrency runtime
//!   implementation (both deferred to Phase 9+).

pub mod anf;
pub mod artifact_manifest;
pub mod cache;
pub mod capabilities;
pub mod compiler_report;
pub mod core_ir;
pub mod error;
pub mod expr_parser;
pub mod hash;
pub mod incremental;
pub mod lower;
pub mod native;
mod native_binding;
mod native_codegen;
mod native_lower;
pub mod native_stub;
mod native_types;
pub mod optimize;
pub mod wasm;
mod wasm_abi;
mod wasm_artifact;
mod wasm_emit;
mod wasm_sections;

#[cfg(test)]
mod spike_v03;

pub use anf::{
    ANF_SCHEMA_VERSION, AnfBinding, AnfExpr, AnfIr, AnfMatchArm, AnfSelectClause, SourceMap,
    SourceMapEntry,
};
pub use artifact_manifest::ArtifactManifest;
pub use cache::{ArtifactCache, ArtifactEntry, MemoryArtifactCache};
pub use capabilities::{CapabilitiesManifest, CapabilityEntry};
pub use compiler_report::{CompilerReport, CompilerWarning, StageRecord};
pub use core_ir::{
    CoreExpr, CoreIr, CoreNode, CoreNodeKind, CoreType, LiteralValue, MatchArm, SelectClause,
    StageHashes,
};
pub use error::CompileError;
pub use expr_parser::{ParseError, parse_expr};
pub use incremental::{DirtySet, NodeHashes, compile_incremental, compute_node_hashes};
pub use lower::{
    is_report_accepted, lower_core_expr_to_anf, lower_to_anf, lower_to_anf_with_graph,
    lower_to_core_ir, nominal_to_core_type,
};
pub use native::{NativeArtifact, emit_native, emit_native_with_profile};
pub use native_stub::{RUNTIME_SYMBOLS, build_runtime_stub_archive, build_runtime_stub_object};
pub use optimize::optimize_bindings;
pub use wasm::{WasmArtifact, emit_wasm, emit_wasm_with_profile};
