use super::*;

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
pub(super) fn expand_infer_boundary(ops: Vec<CanonicalOp>) -> Vec<CanonicalOp> {
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
