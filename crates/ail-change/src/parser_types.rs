// ── ail-change::parser_types ─────────────────────────────────────────────
//
// Public data transfer objects (DTOs) produced by the ACL parser.
//
// These types are kept separate from the parser implementation to reduce
// pressure on `parser.rs` and to allow other modules to import parser DTOs
// without pulling in parser internals.
//
// All types in this module are re-exported from `parser` for backward
// compatibility: existing `use crate::parser::ParsedChangeSet` paths
// continue to work unchanged.

use std::collections::BTreeMap;

use crate::{
    canonical::Precondition,
    model::{ChangeSet, ChangeSetMeta, ChangeSetOp, SnapshotId, Timestamp},
};

// ── OpArgs ────────────────────────────────────────────────────────────────

/// Parsed key=value arguments for a single op line.
///
/// Keys and values are kept as strings; semantic validation happens downstream
/// in op-schema validation and the canonicalizer.
pub type OpArgs = BTreeMap<String, String>;

// ── ParsedOp ──────────────────────────────────────────────────────────────

/// A fully parsed op line: verb kind, full verb string, and kv args.
///
/// `kind` is the coarse `ChangeSetOp` variant derived from the verb prefix.
/// `verb` is the complete verb as written (e.g. `"create_function"`).
/// `args` holds every `key=value` pair from the op line, in sorted key order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedOp {
    /// Coarse variant category of the op.
    pub kind: ChangeSetOp,
    /// Full verb as written in the source (e.g. `"create_function"`).
    pub verb: String,
    /// Key/value arguments from the op line.
    pub args: OpArgs,
}

// ── ChangeComposition ─────────────────────────────────────────────────────

/// Cross-changeset composition relationships.
///
/// All fields default to empty vecs; absent directives produce no entries.
#[derive(Clone, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ChangeComposition {
    /// ChangeSet IDs that must be applied before this one.
    pub depends_on: Vec<String>,
    /// ChangeSet IDs superseded by this one.
    pub supersedes: Vec<String>,
    /// ChangeSet IDs that conflict with this one (require resolution).
    pub conflicts_with: Vec<String>,
    /// Parent epic or umbrella changeset this belongs to.
    pub part_of: Vec<String>,
    /// ChangeSet IDs blocked from applying until this one resolves.
    pub blocks: Vec<String>,
}

// ── ParsedBlock ───────────────────────────────────────────────────────────

/// A typed block section (`block <kind> @ref ... end`).
///
/// Blocks carry free-form content (expressions, schemas, docs, ranges) that
/// is parsed by a subgrammar downstream. The canonicalizer carries them
/// through intact and records their blake3 hash.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ParsedBlock {
    /// Block type: `expr`, `schema`, `doc`, `range`, `policy`, `test`.
    pub kind: String,
    /// The `@ref` identifier for this block (e.g. `@expr.checkout_body`).
    pub block_ref: String,
    /// Raw content lines between the block header and `end`.
    pub content: String,
    /// Optional hash declared inline on the block header (`hash=<value>`).
    pub hash: Option<String>,
}

// ── ExpectClaims ─────────────────────────────────────────────────────────

/// Claims about the expected diff, written by the LLM in the `expect` section.
///
/// These are verifiable claims (not authoritative policy) — the toolchain
/// checks them against the actual structural diff.
#[derive(Clone, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ExpectClaims(pub Vec<String>);

// ── ApprovalRequirements ─────────────────────────────────────────────────

/// Approval requirements declared in the `approval` section.
///
/// Each string is a raw `require_if <condition>` directive.
#[derive(Clone, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ApprovalRequirements(pub Vec<String>);

// ── ParsedChangeSet ───────────────────────────────────────────────────────

/// Result of parsing an ACL document.
///
/// Carries the typed `ChangeSet` (ops + metadata) plus any preconditions
/// declared in the `requires` section.  Preconditions are kept separate
/// because `ChangeSet` itself is a pure value type with no precondition
/// field; preconditions are attached during canonicalization.
///
/// `parsed_ops` mirrors `changeset.ops` but carries the full verb and
/// kv args; use these for canonicalization and op-schema validation.
/// `changeset.ops` is preserved for backward compatibility with apply tests.
///
/// ## Schema versions (from doc §Versioning y schema evolution)
///
/// Schemas evolve independently; each field tracks the declared version for
/// the corresponding schema component.  All fields default to `None` when
/// the corresponding directive is absent from the metadata section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedChangeSet {
    /// The typed changeset (ops + metadata + base snapshot).
    pub changeset: ChangeSet,
    /// Preconditions declared in the `requires` section.
    pub preconditions: Vec<Precondition>,
    /// Enriched ops with full verb and kv args (parallel to `changeset.ops`).
    pub parsed_ops: Vec<ParsedOp>,
    /// ACL language syntax version (defaults to `"1.0"`).
    pub acl_version: String,
    /// Op-schema version declared by this changeset (`op_schema <N>`).
    pub op_schema_version: Option<String>,
    /// Semantic Graph schema version (`graph_schema <N>`).
    pub graph_schema_version: Option<String>,
    /// Core IR schema version (`core_ir_schema <N>`).
    pub core_ir_schema_version: Option<String>,
    /// Diagnostics format version (`diagnostics_schema <N>`).
    pub diagnostics_schema_version: Option<String>,
    /// Verification report format version (`verification_schema <N>`).
    pub verification_schema_version: Option<String>,
    /// Claims about the expected diff (from `expect` section).
    pub expect: Option<ExpectClaims>,
    /// Approval requirements (from `approval` section).
    pub approval: Option<ApprovalRequirements>,
    /// Cross-changeset composition relationships (from `metadata` section).
    pub composition: ChangeComposition,
    /// Typed block sections (`block <kind> @ref ... end`).
    pub blocks: Vec<ParsedBlock>,
    /// Verify directives: short form lines and block form lines combined.
    pub verify: Vec<String>,
}

impl Default for ParsedChangeSet {
    fn default() -> Self {
        Self {
            changeset: ChangeSet {
                meta: ChangeSetMeta {
                    author: String::new(),
                    description: String::new(),
                    timestamp: Timestamp(0),
                },
                base_snapshot_id: SnapshotId(0),
                ops: Vec::new(),
            },
            preconditions: Vec::new(),
            parsed_ops: Vec::new(),
            acl_version: "1.0".to_string(),
            op_schema_version: None,
            graph_schema_version: None,
            core_ir_schema_version: None,
            diagnostics_schema_version: None,
            verification_schema_version: None,
            expect: None,
            approval: None,
            composition: ChangeComposition::default(),
            blocks: Vec::new(),
            verify: Vec::new(),
        }
    }
}
