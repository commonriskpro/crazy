use super::*;

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
pub(super) fn canonicalize_parsed_inner(pcs: ParsedChangeSet) -> CanonicalChangeSet {
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
    op_pairs.sort_by_key(|a| phase_order(&a.0));

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
