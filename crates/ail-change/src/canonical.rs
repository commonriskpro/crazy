// ── ail-change::canonical ─────────────────────────────────────────────────
//
// Deterministic canonicalization of a `ChangeSet`.
//
// # Guarantees
//
// 1. **Stable phase order**: Create → Set/Add/Remove → Connect → Infer → Verify.
//    The stable sort preserves relative order among ops of the same phase.
// 2. **Default materialization**: an empty `description` is replaced with
//    `"<no description>"` so downstream consumers never handle empty strings.
//    `create_function` and `create_type` ops without `visibility` get
//    `visibility=private` materialized.
// 3. **Per-block hashing**: every `CanonicalOp` carries a blake3 `BlockHash`
//    computed from the op's CBOR encoding and its position index.
// 4. **Determinism**: calling `canonicalize` / `canonicalize_parsed` twice
//    with the same input always produces CBOR-identical output.
// 5. **Precondition carry-through**: `canonicalize_parsed` carries preconditions
//    from `ParsedChangeSet` into the resulting `CanonicalChangeSet`.
// 6. **ACL version**: `CanonicalChangeSet` records the `acl_version` from the
//    source document (defaults to `"1.0"`).

use std::collections::BTreeMap;

use ail_core::semantic_graph::{
    Assertion, Binding, CapabilityReqs, EdgeKind, GeneratedArtifact, GraphEdge, GraphNode,
    InferredFact, NodeKind, NodeRef, RuntimeCheckMeta, TypeFacts, Visibility, WorkflowState,
};
use serde::{Deserialize, Serialize};

use crate::acl_migrator::{CURRENT_ACL_VERSION, MigrateError, run_migration_chain};
use crate::model::{
    AssertExists, AssertHash, BlockHash, ChangeSet, ChangeSetOp, SnapshotId, Timestamp,
};
use crate::parser::{
    ApprovalRequirements, ChangeComposition, ExpectClaims, OpArgs, ParsedBlock, ParsedChangeSet,
};

// ── Phase ordering ────────────────────────────────────────────────────────

/// Canonical phase ordinal for stable sorting.
///
/// | Phase | Ops |
/// |-------|-----|
/// |     0 | Create |
/// |     1 | Set, Add, Remove, Delete, Disconnect, Rename, Move, Replace |
/// |     2 | Connect, Bind, Expose, Hide, Grant, Revoke |
/// |     3 | Infer, Derive, Generate |
/// |     4 | Assert, Lock, Refactor, Migrate, Approve, Reject, Deprecate, Annotate, Verify |
fn phase_order(op: &ChangeSetOp) -> u8 {
    match op {
        ChangeSetOp::Create => 0,
        ChangeSetOp::Set
        | ChangeSetOp::Add
        | ChangeSetOp::Remove
        | ChangeSetOp::Delete
        | ChangeSetOp::Disconnect
        | ChangeSetOp::Rename
        | ChangeSetOp::Move
        | ChangeSetOp::Replace => 1,
        ChangeSetOp::Connect
        | ChangeSetOp::Bind
        | ChangeSetOp::Expose
        | ChangeSetOp::Hide
        | ChangeSetOp::Grant
        | ChangeSetOp::Revoke => 2,
        ChangeSetOp::Infer | ChangeSetOp::Derive | ChangeSetOp::Generate => 3,
        ChangeSetOp::Assert
        | ChangeSetOp::Lock
        | ChangeSetOp::Refactor
        | ChangeSetOp::Migrate
        | ChangeSetOp::Approve
        | ChangeSetOp::Reject
        | ChangeSetOp::Deprecate
        | ChangeSetOp::Annotate
        | ChangeSetOp::Verify => 4,
    }
}

// ── CanonicalMeta ─────────────────────────────────────────────────────────

/// Canonicalized metadata with all optional fields materialized.
///
/// `description` is guaranteed non-empty: an empty source value becomes
/// `"<no description>"`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalMeta {
    /// Identity of the change author.
    pub author: String,
    /// Human-readable description; always non-empty after canonicalization.
    pub description: String,
    /// When the changeset was created.
    pub timestamp: Timestamp,
}

// ── OpPayload ─────────────────────────────────────────────────────────────

/// Concrete graph mutation payload for a `CanonicalOp`.
///
/// For ops originating from a raw `ChangeSet` (which has no payload data),
/// `canonicalize` produces `Noop`. Apply tests construct `CanonicalOp`s
/// directly with real payloads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpPayload {
    /// Create a new node in the graph.
    CreateNode(Box<GraphNode>),
    /// Add a directed edge to the graph.
    AddEdge(GraphEdge),
    /// Remove an existing node by ref.
    RemoveNode(NodeRef),
    /// Rename a node (minimal Set semantics).
    SetNodeName { node_id: NodeRef, name: String },
    /// Remove an existing node by stable graph name.
    RemoveNodeByName(String),
    /// Rename a node by stable graph name.
    RenameNodeByName { target: String, name: String },
    /// Add a directed edge after resolving endpoint names during apply.
    AddEdgeByName {
        source: String,
        target: String,
        kind: EdgeKind,
    },
    /// Remove a directed edge after resolving endpoint names during apply.
    RemoveEdgeByName {
        source: String,
        target: String,
        kind: EdgeKind,
    },
    /// Set a function return type by stable graph name.
    SetReturnByName { target: String, ty: String },
    /// Set a function expression body by stable graph name.
    SetBodyByName { target: String, body: String },
    /// Set a supported scalar metadata field by stable graph name.
    SetMetadataByName {
        target: String,
        key: String,
        value: String,
    },
    /// Add a function parameter by stable graph name.
    AddParamByName {
        target: String,
        name: String,
        ty: String,
    },
    /// Add an effect to a node's effect row by stable graph name.
    AddEffectByName { target: String, effect: String },
    /// Remove an effect from a node's effect row by stable graph name.
    RemoveEffectByName { target: String, effect: String },
    /// Add a contract clause to a node by stable graph name.
    AddContractByName {
        target: String,
        kind: String,
        rule: String,
    },
    /// Remove a contract clause from a node by stable graph name.
    RemoveContractByName { target: String, rule: String },
    /// Add a capability requirement to a node by stable graph name.
    AddCapabilityReqByName { target: String, capability: String },
    /// Remove a capability requirement from a node by stable graph name.
    RemoveCapabilityReqByName { target: String, capability: String },
    /// Set export visibility by stable graph name.
    SetVisibilityByName {
        target: String,
        visibility: Visibility,
    },
    /// Add a binding to a node by stable graph name.
    AddBindingByName { target: String, binding: Binding },
    /// Add an inferred fact to a node by stable graph name.
    AddInferredFactByName { target: String, fact: InferredFact },
    /// Add a derived implementation to a node by stable graph name.
    AddDerivedImplByName { target: String, impl_name: String },
    /// Add a generated artifact reference to a node by stable graph name.
    AddGeneratedArtifactByName {
        target: String,
        artifact: GeneratedArtifact,
    },
    /// Add a compile-time assertion to a node by stable graph name.
    AddAssertionByName {
        target: String,
        assertion: Assertion,
    },
    /// Set workflow state by stable graph name.
    SetWorkflowStateByName {
        target: String,
        state: WorkflowState,
    },
    /// No-op placeholder; used for raw ChangeSet ops or malformed parsed ops.
    Noop,
}

// ── CanonicalOp ───────────────────────────────────────────────────────────

/// A single canonicalized operation: phase classifier + payload + block hash.
///
/// `verb` holds the full verb string (e.g. `"create_function"`).
/// `args` holds the kv arguments with all applicable defaults materialized.
/// For ops originating from the legacy `canonicalize(ChangeSet)` path,
/// `verb` is the empty string and `args` is empty.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalOp {
    /// Phase classifier (used for ordering and labelling).
    pub kind: ChangeSetOp,
    /// Full verb as written in the source (e.g. `"create_function"`).
    /// Empty string for ops produced by the legacy `canonicalize` path.
    pub verb: String,
    /// Key/value arguments with defaults materialized.
    /// Empty for ops produced by the legacy `canonicalize` path.
    #[serde(default)]
    pub args: OpArgs,
    /// Concrete graph mutation payload.
    pub payload: OpPayload,
    /// blake3 hash of this op's canonical encoding.
    pub block_hash: BlockHash,
}

impl Default for CanonicalOp {
    fn default() -> Self {
        Self {
            kind: ChangeSetOp::Infer,
            verb: String::new(),
            args: BTreeMap::new(),
            payload: OpPayload::Noop,
            block_hash: BlockHash([0u8; 32]),
        }
    }
}

// ── Precondition ──────────────────────────────────────────────────────────

/// A precondition evaluated before ops are applied.
///
/// If any precondition fails, `apply` returns `Failed` and restores the
/// pre-apply graph clone (rollback).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Precondition {
    /// The referenced node must exist in the graph (numeric NodeRef form).
    AssertExists(AssertExists),
    /// The referenced node's canonical hash must match the expected value (numeric form).
    AssertHash(AssertHash),
    /// The named node (e.g. `type.Cart`) must exist in the graph.
    AssertExistsByName(String),
    /// The named node's canonical hash must match the expected value.
    AssertHashByName {
        /// Stable graph name of the node (e.g. `fn.cart_total`).
        name: String,
        /// Expected blake3 hash of the node's canonical encoding.
        expected_hash: BlockHash,
    },
    /// A context slice hash assertion: verifies that the named node exists
    /// and — when a context hash is supplied — that it matches the recorded
    /// context slice hash (tool: `ail context <target>`).
    AssertContext {
        /// Stable graph name of the target (e.g. `fn.checkout`).
        target_name: String,
        /// Optional context hash returned by `ail context <target> --json`.
        context_hash: Option<String>,
    },
}

// ── CanonicalChangeSet ────────────────────────────────────────────────────

/// A fully canonicalized changeset ready for atomic application.
///
/// Constructed either via `canonicalize(ChangeSet)` / `canonicalize_parsed`
/// or directly in tests when explicit payloads are required.
///
/// All optional sections from the submitted form (`expect`, `approval`,
/// `composition`, `blocks`, `verify`) are carried through so downstream
/// consumers (verifier, policy engine) can inspect them without re-parsing
/// the submitted text.
///
/// ## Schema versions (§Versioning y schema evolution)
///
/// The five schema version fields are carried through from `ParsedChangeSet`
/// unchanged. They default to `None` when absent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalChangeSet {
    /// Canonicalized authorship and intent metadata.
    pub meta: CanonicalMeta,
    /// Snapshot identity against which this changeset was authored.
    pub base_snapshot_id: SnapshotId,
    /// ACL language syntax version (defaults to `"1.0"`).
    #[serde(default = "default_acl_version")]
    pub acl_version: String,
    /// Op-schema version declared by this changeset (`op_schema <N>`).
    #[serde(default)]
    pub op_schema_version: Option<String>,
    /// Semantic Graph schema version (`graph_schema <N>`).
    #[serde(default)]
    pub graph_schema_version: Option<String>,
    /// Core IR schema version (`core_ir_schema <N>`).
    #[serde(default)]
    pub core_ir_schema_version: Option<String>,
    /// Diagnostics format version (`diagnostics_schema <N>`).
    #[serde(default)]
    pub diagnostics_schema_version: Option<String>,
    /// Verification report format version (`verification_schema <N>`).
    #[serde(default)]
    pub verification_schema_version: Option<String>,
    /// Preconditions evaluated before any op is applied.
    pub preconditions: Vec<Precondition>,
    /// Phase-ordered, hash-stamped operations.
    pub ops: Vec<CanonicalOp>,
    /// AI-written claims about the expected diff (carried for verifier).
    #[serde(default)]
    pub expect: Option<ExpectClaims>,
    /// Approval requirements declared in the submitted form.
    #[serde(default)]
    pub approval: Option<ApprovalRequirements>,
    /// Cross-changeset composition relationships.
    #[serde(default)]
    pub composition: ChangeComposition,
    /// Typed block sections (expressions, schemas, docs, etc.).
    #[serde(default)]
    pub blocks: Vec<ParsedBlock>,
    /// Verify directives (short form and block form lines combined).
    #[serde(default)]
    pub verify: Vec<String>,
}

impl Default for CanonicalChangeSet {
    fn default() -> Self {
        Self {
            meta: CanonicalMeta {
                author: String::new(),
                description: String::new(),
                timestamp: Timestamp(0),
            },
            base_snapshot_id: SnapshotId(0),
            acl_version: default_acl_version(),
            op_schema_version: None,
            graph_schema_version: None,
            core_ir_schema_version: None,
            diagnostics_schema_version: None,
            verification_schema_version: None,
            preconditions: Vec::new(),
            ops: Vec::new(),
            expect: None,
            approval: None,
            composition: ChangeComposition::default(),
            blocks: Vec::new(),
            verify: Vec::new(),
        }
    }
}

fn default_acl_version() -> String {
    "1.0".to_string()
}

// ── normalize_id ──────────────────────────────────────────────────────────

/// Normalize an ACL identifier to canonical lower_snake form.
///
/// Rules (from spec):
/// - `Fn.CartTotal`    → `fn.cart_total`   (upper namespace + PascalCase → lower.snake)
/// - `fn.cart-total`   → `fn.cart_total`   (kebab → snake)
/// - `fn.cart_total`   → `fn.cart_total`   (already canonical)
/// - `type.CartItem`   → `type.CartItem`   (type. namespace: PascalCase preserved)
/// - `handler.StripePayment` → `handler.StripePayment` (handler. namespace preserved)
///
/// Namespaces that use PascalCase by convention (`type.`, `handler.`,
/// `boundary.`) are left with their original casing in the local part.
/// All other namespaces are lowercased with underscores.
pub fn normalize_id(id: &str) -> String {
    let dot = match id.find('.') {
        Some(p) => p,
        None => return id.to_string(), // no namespace — return as-is
    };
    let ns = &id[..dot];
    let local = &id[dot + 1..];

    // Namespaces whose local part keeps PascalCase by spec convention.
    const PASCAL_NAMESPACES: &[&str] = &["type", "handler", "boundary"];

    let ns_lower = ns.to_lowercase();
    let local_normalized = if PASCAL_NAMESPACES.contains(&ns_lower.as_str()) {
        // Preserve PascalCase in the local part; still normalize kebab → snake.
        local.replace('-', "_")
    } else {
        // Lower namespace + snake_case local part.
        pascal_to_snake(local).replace('-', "_")
    };

    format!("{}.{}", ns_lower, local_normalized)
}

/// Convert a PascalCase string to lower_snake_case.
///
/// `CartTotal` → `cart_total`
/// `cart_total` → `cart_total` (already snake)
fn pascal_to_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, ch) in s.char_indices() {
        if ch.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(ch.to_lowercase());
    }
    out
}

// ── normalize_op_args ─────────────────────────────────────────────────────

/// Normalize ID-valued op arguments in place.
///
/// Keys that conventionally carry node identifiers (`id`, `target`, `source`,
/// `to`, `from`) are normalized via `normalize_id`. Other values (type
/// expressions, free strings) are left as-is.
const ID_ARG_KEYS: &[&str] = &["id", "target", "source", "to", "from"];

fn normalize_op_args(args: &mut OpArgs) {
    for key in ID_ARG_KEYS {
        if let Some(v) = args.get_mut(*key) {
            let normalized = normalize_id(v);
            *v = normalized;
        }
    }
}

// ── canonicalize ──────────────────────────────────────────────────────────

/// Transform a raw `ChangeSet` into its canonical form.
///
/// Steps:
/// 1. Materialize `description`: replace `""` with `"<no description>"`.
/// 2. Stable-sort ops by phase ordinal (see `phase_order`).
/// 3. Compute a blake3 `BlockHash` per op from its CBOR encoding + index.
/// 4. Wrap each op with `OpPayload::Noop` (raw ops carry no payload data).
///
/// This is the legacy path — no kv args, no defaults, no preconditions.
/// Prefer `canonicalize_parsed` when a `ParsedChangeSet` is available.
pub fn canonicalize(cs: ChangeSet) -> CanonicalChangeSet {
    // Step 1: materialize description default.
    let description = if cs.meta.description.is_empty() {
        "<no description>".to_string()
    } else {
        cs.meta.description
    };

    // Step 2: stable-sort ops by canonical phase order.
    let mut sorted_ops = cs.ops;
    sorted_ops.sort_by_key(phase_order);

    // Step 3+4: compute per-block hash and wrap.
    let canonical_ops: Vec<CanonicalOp> = sorted_ops
        .into_iter()
        .enumerate()
        .map(|(idx, op)| {
            let block_hash = compute_block_hash(&op, idx);
            CanonicalOp {
                kind: op,
                verb: String::new(),
                args: BTreeMap::new(),
                payload: OpPayload::Noop,
                block_hash,
            }
        })
        .collect();

    CanonicalChangeSet {
        meta: CanonicalMeta {
            author: cs.meta.author,
            description,
            timestamp: cs.meta.timestamp,
        },
        base_snapshot_id: cs.base_snapshot_id,
        acl_version: "1.0".to_string(),
        op_schema_version: None,
        graph_schema_version: None,
        core_ir_schema_version: None,
        diagnostics_schema_version: None,
        verification_schema_version: None,
        preconditions: vec![],
        ops: canonical_ops,
        expect: None,
        approval: None,
        composition: ChangeComposition::default(),
        blocks: vec![],
        verify: vec![],
    }
}

// ── expand_infer_boundary ─────────────────────────────────────────────────

/// Post-process canonical ops: for every `infer_boundary` op, synthesize
/// explicit `SetReturnByName` and `AddEffectByName` ops so the canonical form
/// is self-contained.
///
/// Sources consulted (in priority order):
/// 1. `type=` / `effect=` args directly on the `infer_boundary` op.
/// 2. `create_function id=<target> return=T` ops in the same changeset.
/// 3. `set_return target=<target> type=T` ops in the same changeset.
///
/// The original `AddInferredFactByName` op is preserved as documentation.
fn expand_infer_boundary(ops: Vec<CanonicalOp>) -> Vec<CanonicalOp> {
    // Collect return-type info from create_function ops only.
    // We intentionally skip explicit set_return ops — those already produce
    // SetReturnByName via materialize_payload and must not be duplicated.
    let return_map: BTreeMap<String, String> = ops
        .iter()
        .filter_map(|op| match (&op.kind, op.verb.as_str()) {
            (ChangeSetOp::Create, "create_function") => {
                let target = op.args.get("id")?.clone();
                let ty = op.args.get("return")?.clone();
                Some((target, ty))
            }
            _ => None,
        })
        .collect();

    // Collect targets that already have an explicit set_return in this changeset
    // so we don't synthesize a duplicate.
    let explicit_return_targets: std::collections::BTreeSet<String> = ops
        .iter()
        .filter_map(|op| {
            if matches!(op.kind, ChangeSetOp::Set) && op.verb == "set_return" {
                op.args.get("target").cloned()
            } else {
                None
            }
        })
        .collect();

    // Find all infer_boundary ops.
    let infer_ops: Vec<CanonicalOp> = ops
        .iter()
        .filter(|op| op.kind == ChangeSetOp::Infer && op.verb == "infer_boundary")
        .cloned()
        .collect();

    let mut extra: Vec<CanonicalOp> = Vec::new();

    for infer_op in infer_ops {
        let Some(target) = infer_op.args.get("target").cloned() else {
            continue;
        };
        let base_idx = ops.len() + extra.len();

        // Synthesize SetReturnByName: from explicit type= arg, then from create_function.
        // Skip if an explicit set_return for this target already exists (avoid duplicates).
        let return_ty = if explicit_return_targets.contains(&target) {
            None
        } else {
            infer_op
                .args
                .get("type")
                .cloned()
                .or_else(|| return_map.get(&target).cloned())
        };
        if let Some(ty) = return_ty {
            let block_hash = compute_block_hash(&ChangeSetOp::Set, base_idx);
            extra.push(CanonicalOp {
                kind: ChangeSetOp::Set,
                verb: "set_return".to_string(),
                args: {
                    let mut a = BTreeMap::new();
                    a.insert("target".to_string(), target.clone());
                    a.insert("type".to_string(), ty.clone());
                    a
                },
                payload: OpPayload::SetReturnByName {
                    target: target.clone(),
                    ty,
                },
                block_hash,
            });
        }

        // Synthesize AddEffectByName: from explicit effect= arg on infer_boundary.
        if let Some(effect) = infer_op.args.get("effect").cloned() {
            let block_hash = compute_block_hash(&ChangeSetOp::Add, base_idx + extra.len());
            extra.push(CanonicalOp {
                kind: ChangeSetOp::Add,
                verb: "add_effect".to_string(),
                args: {
                    let mut a = BTreeMap::new();
                    a.insert("target".to_string(), target.clone());
                    a.insert("effect".to_string(), effect.clone());
                    a
                },
                payload: OpPayload::AddEffectByName {
                    target: target.clone(),
                    effect,
                },
                block_hash,
            });
        }
    }

    let mut result = ops;
    result.extend(extra);
    result
}

// ── canonicalize_parsed ───────────────────────────────────────────────────

/// Transform a `ParsedChangeSet` (with full kv args) into canonical form.
///
/// Additional steps beyond `canonicalize`:
/// - Runs the ACL migrator chain when `acl_version` < [`CURRENT_ACL_VERSION`].
/// - Carries preconditions from `ParsedChangeSet.preconditions`.
/// - Carries `acl_version` (post-migration) from the parsed document.
/// - Carries `expect`, `approval`, `composition`, `blocks`, `verify`.
/// - Materializes safe defaults:
///   - `create_function` / `create_type` without `visibility` → `"private"`.
///   - `create_type` without `derive` → `"none"`.
/// - Normalizes ID-valued args (`id`, `target`, `source`, `to`, `from`).
/// - Marks `infer_*` ops with `infer_pending=true` for downstream expansion.
/// - Stores full verb and materialized args on each `CanonicalOp`.
/// - **Expands `infer_boundary`** into explicit `set_return` / `add_effect` ops
///   so the canonical form is self-contained (see `expand_infer_boundary`).
///
/// # Panics
///
/// Panics if the document declares an ACL version for which no migration path
/// is registered.  Use [`try_canonicalize_parsed`] to handle unknown versions
/// without panicking.
pub fn canonicalize_parsed(pcs: ParsedChangeSet) -> CanonicalChangeSet {
    try_canonicalize_parsed(pcs).expect("ACL migration must succeed for known versions")
}

/// Fallible variant of [`canonicalize_parsed`].
///
/// Returns `Ok(CanonicalChangeSet)` on success or
/// `Err(MigrateError::UnknownVersion)` when the document declares a version
/// for which no migration path is registered.
pub fn try_canonicalize_parsed(pcs: ParsedChangeSet) -> Result<CanonicalChangeSet, MigrateError> {
    // Run the migration chain if the declared version is not current.
    let pcs = if pcs.acl_version != CURRENT_ACL_VERSION {
        run_migration_chain(pcs, CURRENT_ACL_VERSION)?
    } else {
        pcs
    };
    Ok(canonicalize_parsed_inner(pcs))
}

/// Inner, infallible canonicalization after migration has already run.
fn canonicalize_parsed_inner(pcs: ParsedChangeSet) -> CanonicalChangeSet {
    // Step 1: materialize description default.
    let description = if pcs.changeset.meta.description.is_empty() {
        "<no description>".to_string()
    } else {
        pcs.changeset.meta.description.clone()
    };

    // Step 2: zip parsed_ops with kinds for sorting; fall back to bare kind
    // if parsed_ops is shorter (shouldn't happen in normal flow).
    let mut op_pairs: Vec<(ChangeSetOp, String, OpArgs)> = if pcs.parsed_ops.is_empty() {
        // Legacy path: no parsed ops, just bare kinds.
        pcs.changeset
            .ops
            .into_iter()
            .map(|k| (k, String::new(), BTreeMap::new()))
            .collect()
    } else {
        pcs.parsed_ops
            .into_iter()
            .map(|po| (po.kind, po.verb, po.args))
            .collect()
    };

    // Stable-sort by canonical phase order.
    op_pairs.sort_by(|a, b| phase_order(&a.0).cmp(&phase_order(&b.0)));

    // Materialize defaults, normalize IDs, mark infer ops, compute hashes.
    let canonical_ops: Vec<CanonicalOp> = op_pairs
        .into_iter()
        .enumerate()
        .map(|(idx, (kind, verb, mut args))| {
            // Normalize ID-valued arguments.
            normalize_op_args(&mut args);
            // Materialize safe defaults for this op.
            materialize_defaults(&kind, &verb, &mut args);
            // Mark infer_* verbs as pending expansion.
            if kind == ChangeSetOp::Infer && verb.starts_with("infer_") {
                args.entry("infer_pending".to_string())
                    .or_insert_with(|| "true".to_string());
            }
            let block_hash = compute_block_hash(&kind, idx);
            let payload = materialize_payload(idx, &kind, &verb, &args);
            CanonicalOp {
                kind,
                verb,
                args,
                payload,
                block_hash,
            }
        })
        .collect();

    // Expand infer_boundary ops: synthesize explicit set_return / add_effect ops
    // so the canonical form is self-contained.
    let canonical_ops = expand_infer_boundary(canonical_ops);

    CanonicalChangeSet {
        meta: CanonicalMeta {
            author: pcs.changeset.meta.author,
            description,
            timestamp: pcs.changeset.meta.timestamp,
        },
        base_snapshot_id: pcs.changeset.base_snapshot_id,
        acl_version: pcs.acl_version,
        op_schema_version: pcs.op_schema_version,
        graph_schema_version: pcs.graph_schema_version,
        core_ir_schema_version: pcs.core_ir_schema_version,
        diagnostics_schema_version: pcs.diagnostics_schema_version,
        verification_schema_version: pcs.verification_schema_version,
        preconditions: pcs.preconditions,
        ops: canonical_ops,
        expect: pcs.expect,
        approval: pcs.approval,
        composition: pcs.composition,
        blocks: pcs.blocks,
        verify: pcs.verify,
    }
}

// ── materialize_defaults ──────────────────────────────────────────────────

/// Apply safe, mechanical defaults to an op's args in place.
///
/// Defaults applied:
/// - `create_function` / `create_type` without `visibility` → `"private"`.
/// - `create_type` without `derive` → `"none"`.
///
/// Rules (from spec):
/// 1. Defaults must be safe (never public/unsafe/assumed).
/// 2. Defaults must be mechanical (no semantic ambiguity).
/// 3. Defaults must not grant permissions or expose APIs.
fn materialize_defaults(kind: &ChangeSetOp, verb: &str, args: &mut OpArgs) {
    match kind {
        ChangeSetOp::Create => {
            if matches!(verb, "create_function" | "create_type") {
                args.entry("visibility".to_string())
                    .or_insert_with(|| "private".to_string());
            }
            if verb == "create_type" {
                args.entry("derive".to_string())
                    .or_insert_with(|| "none".to_string());
            }
        }
        _ => {}
    }
}

fn materialize_payload(idx: usize, kind: &ChangeSetOp, verb: &str, args: &OpArgs) -> OpPayload {
    match (kind, verb) {
        (ChangeSetOp::Create, "create_module") => args
            .get("id")
            .map(|id| OpPayload::CreateNode(Box::new(module_node(idx, id))))
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Create, "create_type") => args
            .get("id")
            .map(|id| OpPayload::CreateNode(Box::new(type_node(idx, id))))
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Create, "create_function") => args
            .get("id")
            .map(|id| OpPayload::CreateNode(Box::new(function_node(idx, id, args))))
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Create, "create_capability") => args
            .get("id")
            .map(|id| OpPayload::CreateNode(Box::new(capability_node(idx, id))))
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Set, "set_return") => target_and(args, "type")
            .map(|(target, ty)| OpPayload::SetReturnByName { target, ty })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Set, "set_body") => target_and(args, "body")
            .map(|(target, body)| OpPayload::SetBodyByName { target, body })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Set, _) | (ChangeSetOp::Replace, _) => args
            .get("target")
            .and_then(|target| metadata_arg(args).map(|(key, value)| (target.clone(), key, value)))
            .map(|(target, key, value)| OpPayload::SetMetadataByName { target, key, value })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Add, "add_param") => {
            match (args.get("target"), args.get("name"), args.get("type")) {
                (Some(target), Some(name), Some(ty)) => OpPayload::AddParamByName {
                    target: target.clone(),
                    name: name.clone(),
                    ty: ty.clone(),
                },
                _ => OpPayload::Noop,
            }
        }
        (ChangeSetOp::Add, "add_effect") => target_and(args, "effect")
            .map(|(target, effect)| OpPayload::AddEffectByName { target, effect })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Add, "add_contract") => {
            match (args.get("target"), args.get("kind"), args.get("rule")) {
                (Some(target), Some(kind), Some(rule)) => OpPayload::AddContractByName {
                    target: target.clone(),
                    kind: kind.clone(),
                    rule: rule.clone(),
                },
                _ => OpPayload::Noop,
            }
        }
        (ChangeSetOp::Remove, "remove_effect") => target_and(args, "effect")
            .map(|(target, effect)| OpPayload::RemoveEffectByName { target, effect })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Remove, "remove_contract") => target_and(args, "rule")
            .map(|(target, rule)| OpPayload::RemoveContractByName { target, rule })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Remove, verb) if verb.starts_with("remove_") => args
            .get("target")
            .cloned()
            .map(OpPayload::RemoveNodeByName)
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Delete, _) => args
            .get("target")
            .cloned()
            .map(OpPayload::RemoveNodeByName)
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Rename, _) => match (args.get("target"), args.get("name")) {
            (Some(target), Some(name)) => OpPayload::RenameNodeByName {
                target: target.clone(),
                name: name.clone(),
            },
            _ => OpPayload::Noop,
        },
        (ChangeSetOp::Move, _) => target_and(args, "to")
            .map(|(target, value)| OpPayload::SetMetadataByName {
                target,
                key: "module".to_string(),
                value,
            })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Connect, _) => edge_payload(args, false),
        (ChangeSetOp::Disconnect, _) => edge_payload(args, true),
        (ChangeSetOp::Grant, _) => target_and(args, "capability")
            .map(|(target, capability)| OpPayload::AddCapabilityReqByName { target, capability })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Revoke, _) => target_and(args, "capability")
            .map(|(target, capability)| OpPayload::RemoveCapabilityReqByName { target, capability })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Expose, _) => args
            .get("target")
            .cloned()
            .map(|target| OpPayload::SetVisibilityByName {
                target,
                visibility: Visibility::Public,
            })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Hide, _) => args
            .get("target")
            .cloned()
            .map(|target| OpPayload::SetVisibilityByName {
                target,
                visibility: Visibility::Private,
            })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Bind, _) => match (args.get("capability"), args.get("handler")) {
            (Some(capability), Some(handler)) => OpPayload::AddBindingByName {
                target: capability.clone(),
                binding: Binding {
                    name: capability.clone(),
                    implementation: handler.clone(),
                    profile: args.get("profile").cloned(),
                },
            },
            _ => OpPayload::Noop,
        },
        (ChangeSetOp::Infer, _) => args
            .get("target")
            .cloned()
            .map(|target| OpPayload::AddInferredFactByName {
                target,
                fact: InferredFact {
                    kind: verb_suffix(verb, "infer"),
                    value: inferred_value(args),
                },
            })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Derive, _) => args
            .get("target")
            .cloned()
            .map(|target| OpPayload::AddDerivedImplByName {
                target,
                impl_name: verb_suffix(verb, "derive"),
            })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Generate, _) => args
            .get("target")
            .cloned()
            .map(|target| OpPayload::AddGeneratedArtifactByName {
                target,
                artifact: GeneratedArtifact {
                    kind: verb_suffix(verb, "generate"),
                    source: generated_source(args),
                },
            })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Assert, _) => args
            .get("target")
            .cloned()
            .map(|target| OpPayload::AddAssertionByName {
                target,
                assertion: Assertion {
                    kind: verb_suffix(verb, "assert"),
                    value: assertion_value(args),
                },
            })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Lock, _) => workflow_payload(args, WorkflowState::Locked),
        (ChangeSetOp::Refactor, _) => workflow_target(args).map_or(OpPayload::Noop, |target| {
            OpPayload::SetWorkflowStateByName {
                target,
                state: WorkflowState::Refactoring,
            }
        }),
        (ChangeSetOp::Migrate, _) => workflow_payload(args, WorkflowState::Migrating),
        (ChangeSetOp::Approve, _) => workflow_payload(args, WorkflowState::Approved),
        (ChangeSetOp::Reject, _) => workflow_payload(args, WorkflowState::Rejected),
        (ChangeSetOp::Verify, _) => workflow_payload(args, WorkflowState::Verified),
        (ChangeSetOp::Deprecate, _) => target_and(args, "replacement")
            .map(|(target, value)| OpPayload::SetMetadataByName {
                target,
                key: "deprecated_replacement".to_string(),
                value,
            })
            .unwrap_or(OpPayload::Noop),
        (ChangeSetOp::Annotate, _) => {
            match (args.get("target"), args.get("key"), args.get("value")) {
                (Some(target), Some(key), Some(value)) => OpPayload::SetMetadataByName {
                    target: target.clone(),
                    key: key.clone(),
                    value: value.clone(),
                },
                _ => OpPayload::Noop,
            }
        }
        // Raw or malformed ops without enough graph identity remain no-ops.
        _ => OpPayload::Noop,
    }
}

fn verb_suffix(verb: &str, prefix: &str) -> String {
    verb.strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix('_'))
        .filter(|rest| !rest.is_empty())
        .unwrap_or(prefix)
        .to_string()
}

fn inferred_value(args: &OpArgs) -> String {
    args.get("type")
        .or_else(|| args.get("effect"))
        .or_else(|| args.get("value"))
        .cloned()
        .unwrap_or_else(|| "pending".to_string())
}

fn generated_source(args: &OpArgs) -> Option<String> {
    args.get("from")
        .or_else(|| args.get("language"))
        .or_else(|| args.get("audience"))
        .cloned()
}

fn assertion_value(args: &OpArgs) -> String {
    args.get("hash")
        .or_else(|| args.get("value"))
        .cloned()
        .unwrap_or_else(|| "true".to_string())
}

fn workflow_payload(args: &OpArgs, state: WorkflowState) -> OpPayload {
    workflow_target(args).map_or(OpPayload::Noop, |target| {
        OpPayload::SetWorkflowStateByName { target, state }
    })
}

fn workflow_target(args: &OpArgs) -> Option<String> {
    args.get("target")
        .or_else(|| args.get("from"))
        .or_else(|| args.get("scope"))
        .cloned()
}

fn target_and(args: &OpArgs, key: &str) -> Option<(String, String)> {
    Some((args.get("target")?.clone(), args.get(key)?.clone()))
}

fn metadata_arg(args: &OpArgs) -> Option<(String, String)> {
    args.iter()
        .find(|(key, _)| !matches!(key.as_str(), "target" | "source" | "id"))
        .map(|(key, value)| (key.clone(), value.clone()))
}

fn edge_payload(args: &OpArgs, remove: bool) -> OpPayload {
    match (args.get("source"), args.get("target")) {
        (Some(source), Some(target)) => {
            let kind = args
                .get("relation")
                .map(|relation| edge_kind(relation))
                .unwrap_or(EdgeKind::DependsOn);
            if remove {
                OpPayload::RemoveEdgeByName {
                    source: source.clone(),
                    target: target.clone(),
                    kind,
                }
            } else {
                OpPayload::AddEdgeByName {
                    source: source.clone(),
                    target: target.clone(),
                    kind,
                }
            }
        }
        _ => OpPayload::Noop,
    }
}

fn edge_kind(relation: &str) -> EdgeKind {
    match relation {
        "calls" => EdgeKind::Calls,
        "reads" => EdgeKind::Reads,
        "writes" => EdgeKind::Writes,
        "emits" => EdgeKind::Emits,
        "proves" => EdgeKind::Proves,
        "breaks_if_changed" => EdgeKind::BreaksIfChanged,
        _ => EdgeKind::DependsOn,
    }
}

fn module_node(idx: usize, id: &str) -> GraphNode {
    GraphNode::new(NodeRef(idx as u32), NodeKind::Module, id)
}

fn type_node(idx: usize, id: &str) -> GraphNode {
    let mut node = GraphNode::new(NodeRef(idx as u32), NodeKind::Type, id);
    node.type_facts = Some(TypeFacts {
        nominal: id.rsplit('.').next().unwrap_or(id).to_string(),
        generics: vec![],
    });
    node
}

fn function_node(idx: usize, id: &str, args: &OpArgs) -> GraphNode {
    let mut node = GraphNode::new(NodeRef(idx as u32), NodeKind::Function, id);
    node.return_type = args.get("return").cloned();
    node.body_expr = args.get("body").cloned();
    if let Some(value) = args.get("value") {
        let predicate = format!("literal:i64={value}");
        node.runtime_checks = Some(vec![RuntimeCheckMeta {
            hash: blake3::hash(predicate.as_bytes()).to_hex().to_string(),
            predicate,
        }]);
    }
    node
}

fn capability_node(idx: usize, id: &str) -> GraphNode {
    let mut node = GraphNode::new(NodeRef(idx as u32), NodeKind::Capability, id);
    node.capability_reqs = Some(CapabilityReqs {
        caps: vec![id.to_string()],
    });
    node
}

// ── helpers ───────────────────────────────────────────────────────────────

/// Compute blake3 hash of `(op CBOR encoding | phase ordinal | index)`.
///
/// The index ensures two identical ops at different positions produce
/// distinct hashes, providing per-block uniqueness.
fn compute_block_hash(op: &ChangeSetOp, idx: usize) -> BlockHash {
    let mut op_bytes: Vec<u8> = Vec::new();
    ciborium::into_writer(op, &mut op_bytes).expect("ChangeSetOp serialization must not fail");

    let mut hasher = blake3::Hasher::new();
    hasher.update(&op_bytes);
    hasher.update(&phase_order(op).to_le_bytes());
    hasher.update(&(idx as u64).to_le_bytes());

    BlockHash(*hasher.finalize().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_changeset;
    use ail_core::semantic_graph::{EdgeKind, NodeKind, Visibility, WorkflowState};

    fn minimal_change(op: &str) -> String {
        format!("change e2e base=0\nauthor tester\ndescription e2e\nop {op}\nend\n")
    }

    #[test]
    fn canonicalize_parsed_materializes_function_create_payload() {
        let parsed = parse_changeset(&minimal_change(
            "create_function id=fn.answer return=Int value=42",
        ))
        .expect("fixture must parse");

        let canonical = canonicalize_parsed(parsed);

        match &canonical.ops[0].payload {
            OpPayload::CreateNode(node) => {
                assert_eq!(node.kind, NodeKind::Function);
                assert_eq!(node.name, "fn.answer");
                assert_eq!(node.return_type.as_deref(), Some("Int"));
                assert_eq!(
                    node.runtime_checks
                        .as_ref()
                        .map(|checks| checks[0].predicate.as_str()),
                    Some("literal:i64=42")
                );
            }
            other => panic!("expected function CreateNode payload, got {other:?}"),
        }
    }

    #[test]
    fn canonicalize_parsed_materializes_function_body_expr_on_create() {
        let parsed = parse_changeset(&minimal_change(
            "create_function id=fn.add return=Int body=add(x, y)",
        ))
        .expect("fixture must parse");

        let canonical = canonicalize_parsed(parsed);

        match &canonical.ops[0].payload {
            OpPayload::CreateNode(node) => {
                assert_eq!(node.body_expr.as_deref(), Some("add(x, y)"));
            }
            other => panic!("expected function CreateNode payload, got {other:?}"),
        }
    }

    #[test]
    fn canonicalize_parsed_materializes_type_create_payload() {
        let parsed = parse_changeset(&minimal_change("create_type id=type.Answer"))
            .expect("fixture must parse");

        let canonical = canonicalize_parsed(parsed);

        match &canonical.ops[0].payload {
            OpPayload::CreateNode(node) => {
                assert_eq!(node.kind, NodeKind::Type);
                assert_eq!(node.name, "type.Answer");
                assert_eq!(
                    node.type_facts.as_ref().map(|facts| facts.nominal.as_str()),
                    Some("Answer")
                );
            }
            other => panic!("expected type CreateNode payload, got {other:?}"),
        }
    }

    #[test]
    fn canonicalize_parsed_materializes_representable_op_payloads() {
        let source = "\
change e2e base=0
author tester
description e2e
op create_module id=module.checkout
op create_capability id=cap.payment.charge
op create_function id=fn.checkout
op add_param target=fn.checkout name=cart_id type=CartId
op set_return target=fn.checkout type=OrderId
op set_body target=fn.checkout body=@expr.checkout
op add_effect target=fn.checkout effect=payment.charge
op add_contract target=fn.checkout kind=ensures rule=order_created
op connect source=fn.checkout relation=uses target=cap.payment.charge
op disconnect source=fn.checkout relation=uses target=cap.payment.charge
op grant target=module.checkout capability=payment.charge
op revoke target=module.checkout capability=payment.charge
op rename target=fn.checkout name=fn.checkout_v2
op move target=fn.checkout_v2 to=module.checkout
op deprecate target=fn.checkout_v2 replacement=fn.checkout_v3
op annotate target=fn.checkout_v2 key=rationale value=idempotent
op remove_effect target=fn.checkout_v2 effect=payment.charge
op remove_contract target=fn.checkout_v2 rule=order_created
op delete target=fn.checkout_v2
end
";
        let canonical = canonicalize_parsed(parse_changeset(source).expect("fixture must parse"));

        assert!(matches!(canonical.ops[0].payload, OpPayload::CreateNode(_)));
        assert!(
            canonical
                .ops
                .iter()
                .any(|op| matches!(op.payload, OpPayload::AddParamByName { .. }))
        );
        assert!(
            canonical
                .ops
                .iter()
                .any(|op| matches!(op.payload, OpPayload::SetReturnByName { .. }))
        );
        assert!(
            canonical
                .ops
                .iter()
                .any(|op| matches!(op.payload, OpPayload::AddEffectByName { .. }))
        );
        assert!(
            canonical
                .ops
                .iter()
                .any(|op| matches!(op.payload, OpPayload::AddContractByName { .. }))
        );
        assert!(canonical.ops.iter().any(|op| matches!(
            op.payload,
            OpPayload::AddEdgeByName {
                kind: EdgeKind::DependsOn,
                ..
            }
        )));
        assert!(
            canonical
                .ops
                .iter()
                .any(|op| matches!(op.payload, OpPayload::RemoveEdgeByName { .. }))
        );
        assert!(
            canonical
                .ops
                .iter()
                .any(|op| matches!(op.payload, OpPayload::AddCapabilityReqByName { .. }))
        );
        assert!(
            canonical
                .ops
                .iter()
                .any(|op| matches!(op.payload, OpPayload::RemoveCapabilityReqByName { .. }))
        );
        assert!(
            canonical
                .ops
                .iter()
                .any(|op| matches!(op.payload, OpPayload::RenameNodeByName { .. }))
        );
        assert!(
            canonical
                .ops
                .iter()
                .any(|op| matches!(op.payload, OpPayload::SetBodyByName { .. }))
        );
        assert!(
            canonical
                .ops
                .iter()
                .any(|op| matches!(op.payload, OpPayload::RemoveEffectByName { .. }))
        );
        assert!(
            canonical
                .ops
                .iter()
                .any(|op| matches!(op.payload, OpPayload::RemoveContractByName { .. }))
        );
        assert!(
            canonical
                .ops
                .iter()
                .any(|op| matches!(op.payload, OpPayload::RemoveNodeByName(_)))
        );
    }

    // ── Gap 1: infer_boundary materialization tests ───────────────────────

    // Scenario IB-1: infer_boundary with explicit type arg materializes set_return op.
    //   GIVEN a changeset with `op infer_boundary target=fn.checkout type=OrderId`
    //   WHEN canonicalize_parsed is called
    //   THEN the canonical ops include both AddInferredFactByName AND SetReturnByName
    #[test]
    fn infer_boundary_with_type_emits_set_return_op() {
        let parsed = parse_changeset(&minimal_change(
            "infer_boundary target=fn.checkout type=OrderId",
        ))
        .expect("fixture must parse");

        let canonical = canonicalize_parsed(parsed);

        assert!(
            canonical
                .ops
                .iter()
                .any(|op| matches!(op.payload, OpPayload::AddInferredFactByName { .. })),
            "must retain AddInferredFactByName as documentation"
        );
        assert!(
            canonical.ops.iter().any(
                |op| matches!(op.payload, OpPayload::SetReturnByName { ref target, ref ty }
                    if target == "fn.checkout" && ty == "OrderId")
            ),
            "must emit explicit SetReturnByName for the inferred return type"
        );
    }

    // TRIANGULATE: infer_boundary with explicit effect arg materializes add_effect op.
    #[test]
    fn infer_boundary_with_effect_emits_add_effect_op() {
        let parsed = parse_changeset(&minimal_change(
            "infer_boundary target=fn.checkout effect=payment.charge",
        ))
        .expect("fixture must parse");

        let canonical = canonicalize_parsed(parsed);

        assert!(
            canonical.ops.iter().any(
                |op| matches!(op.payload, OpPayload::AddEffectByName { ref target, ref effect }
                    if target == "fn.checkout" && effect == "payment.charge")
            ),
            "must emit explicit AddEffectByName for the inferred effect"
        );
    }

    // Scenario IB-2: infer_boundary picks up return type from create_function in same changeset.
    //   GIVEN a changeset with create_function + infer_boundary for the same target
    //   WHEN canonicalize_parsed is called
    //   THEN canonical ops include SetReturnByName derived from create_function's return arg
    #[test]
    fn infer_boundary_derives_return_from_create_function() {
        let source = "\
change e2e base=0
author tester
description e2e
op create_function id=fn.checkout return=OrderId
op infer_boundary target=fn.checkout
end
";
        let canonical = canonicalize_parsed(parse_changeset(source).expect("fixture must parse"));

        assert!(
            canonical.ops.iter().any(
                |op| matches!(op.payload, OpPayload::SetReturnByName { ref target, ref ty }
                    if target == "fn.checkout" && ty == "OrderId")
            ),
            "must emit SetReturnByName derived from create_function return"
        );
    }

    // TRIANGULATE: infer_boundary picks up effects from add_effect ops in same changeset.
    #[test]
    fn infer_boundary_derives_effects_from_add_effect_ops() {
        let source = "\
change e2e base=0
author tester
description e2e
op create_function id=fn.checkout return=OrderId
op add_effect target=fn.checkout effect=payment.charge
op infer_boundary target=fn.checkout
end
";
        let canonical = canonicalize_parsed(parse_changeset(source).expect("fixture must parse"));

        // The AddInferredFactByName must still be present as documentation.
        assert!(
            canonical
                .ops
                .iter()
                .any(|op| matches!(op.payload, OpPayload::AddInferredFactByName { .. })),
            "must retain AddInferredFactByName"
        );
        // An explicit AddEffectByName must be synthesized (from add_effect scanning).
        assert!(
            canonical.ops.iter().any(|op| matches!(
                op.payload,
                OpPayload::AddEffectByName { ref target, ref effect }
                    if target == "fn.checkout" && effect == "payment.charge"
            )),
            "must emit AddEffectByName derived from sibling add_effect op"
        );
    }

    #[test]
    fn canonicalize_parsed_materializes_semantic_graph_payloads() {
        let source = "\
change e2e base=0
author tester
description e2e
op infer_boundary target=fn.checkout
op bind_handler capability=payment.charge handler=handler.Stripe profile=prod
op expose target=fn.checkout as=api.checkout
op hide target=fn.internal
op derive_eq target=type.Address mode=structural
op generate_tests target=fn.checkout from=contracts
op assert_exists target=fn.checkout
op lock_behavior target=fn.checkout
op refactor_inline target=fn.old_helper
op migrate_api target=fn.checkout from=sig.v1 to=sig.v2
op approve_inferred_boundary target=fn.checkout version=sig_123
op reject_inferred_boundary target=fn.checkout version=sig_124
op verify target=fn.checkout
end
";
        let canonical = canonicalize_parsed(parse_changeset(source).expect("fixture must parse"));

        assert!(
            canonical
                .ops
                .iter()
                .any(|op| matches!(op.payload, OpPayload::AddInferredFactByName { .. }))
        );
        assert!(
            canonical
                .ops
                .iter()
                .any(|op| matches!(op.payload, OpPayload::AddBindingByName { .. }))
        );
        assert!(canonical.ops.iter().any(|op| matches!(
            op.payload,
            OpPayload::SetVisibilityByName {
                visibility: Visibility::Public,
                ..
            }
        )));
        assert!(canonical.ops.iter().any(|op| matches!(
            op.payload,
            OpPayload::SetVisibilityByName {
                visibility: Visibility::Private,
                ..
            }
        )));
        assert!(
            canonical
                .ops
                .iter()
                .any(|op| matches!(op.payload, OpPayload::AddDerivedImplByName { .. }))
        );
        assert!(
            canonical
                .ops
                .iter()
                .any(|op| matches!(op.payload, OpPayload::AddGeneratedArtifactByName { .. }))
        );
        assert!(
            canonical
                .ops
                .iter()
                .any(|op| matches!(op.payload, OpPayload::AddAssertionByName { .. }))
        );
        for state in [
            WorkflowState::Locked,
            WorkflowState::Refactoring,
            WorkflowState::Migrating,
            WorkflowState::Approved,
            WorkflowState::Rejected,
            WorkflowState::Verified,
        ] {
            assert!(canonical.ops.iter().any(|op| matches!(
                op.payload,
                OpPayload::SetWorkflowStateByName { state: actual, .. } if actual == state
            )));
        }
        assert!(
            canonical
                .ops
                .iter()
                .all(|op| !matches!(op.payload, OpPayload::Noop))
        );
    }
}
