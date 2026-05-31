// ── ail-compiler::core_ir ─────────────────────────────────────────────────
//
// Core IR value types — the first lowering stage output.
//
// # Design constraints
//
// - `Vec` and `BTreeMap` only (no `HashMap`) — workspace determinism contract.
// - All types `#[derive(Serialize, Deserialize)]` for CBOR hash sealing and
//   round-trip determinism.
// - `CoreIr` owns the `StageHashes` accumulator so later stages can read
//   predecessor hashes without re-computing them.
//
// # G2 scope (core-ir-full)
//
// Extends `CoreNode` with two optional fields:
// - `ty: Option<CoreType>` — the ~20 type primitives from `docs/core-ir.md §3`.
// - `expr: Option<CoreExpr>` — the ~15 expression primitives from §5.
//
// Both fields use `#[serde(default, skip_serializing_if = "Option::is_none")]`
// to keep pre-G2 CBOR wire format byte-identical when the fields are absent.

// ── Submodules ────────────────────────────────────────────────────────────

pub(crate) mod diagnostics;
pub(crate) mod expr;
pub(crate) mod nodes;
pub(crate) mod primitives;
pub(crate) mod types;

// ── Re-exports — public API surface is unchanged ─────────────────────────

pub use diagnostics::{
    CoreIrDiagnostic, CoreIrDiagnosticIssue, CoreIrIssueCategory, CoreIrIssueCode,
};
pub use expr::{CoreExpr, MatchArm, SelectClause};
pub use nodes::{CoreIr, CoreNode, StageHashes};
pub use primitives::{CoreNodeKind, LiteralValue, LoopTermination, ResourceMode};
pub use types::CoreType;

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
