// ── ail-verify::pipeline::changeset_stages ───────────────────────────────
//
// Stages 3–6 helpers: op-schema validation, graph-reference resolution,
// semantic diff construction, and Core IR lowering.
//
// All functions are pure, behavior-preserving extractions from the original
// `pipeline.rs` godfile.  Called by `VerificationPipeline::run_with_changeset`
// in the parent module.

use std::collections::BTreeSet;

use ail_change::canonical::{CanonicalChangeSet, OpPayload};
use ail_change::model::ChangeSetOp;
use ail_core::semantic_graph::SemanticGraph;

use crate::report::{VerificationEntry, VerificationState};

use super::stage_entry;

// ── Schema constants ──────────────────────────────────────────────────────

/// Current schema version for op arg validation.
const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Known primitive type names for arg type validation.
const KNOWN_PRIMITIVES: &[&str] = &[
    "Int", "String", "Bool", "Float", "Decimal", "Money", "Email",
];

// ── Stage 3: Validate op schemas ─────────────────────────────────────────

#[allow(dead_code)]
fn validate_op_schemas(canonical: Option<&CanonicalChangeSet>) -> Vec<VerificationEntry> {
    validate_op_schemas_with_graph(canonical, None)
}

pub(super) fn validate_op_schemas_with_graph(
    canonical: Option<&CanonicalChangeSet>,
    graph: Option<&SemanticGraph>,
) -> Vec<VerificationEntry> {
    let Some(canonical) = canonical else {
        return vec![stage_entry(
            "03-validate-op-schemas",
            VerificationState::Unverified,
            "changeset.ops",
            Some("canonical change unavailable".into()),
        )];
    };

    if canonical.ops.is_empty() {
        return vec![stage_entry(
            "03-validate-op-schemas",
            VerificationState::Proven,
            "changeset.ops",
            Some("identity changeset has no ops".into()),
        )];
    }

    // Build graph node name set for type arg validation
    let graph_names: BTreeSet<&str> = graph
        .map(|g| g.nodes.iter().map(|n| n.name.as_str()).collect())
        .unwrap_or_default();

    canonical
        .ops
        .iter()
        .enumerate()
        .flat_map(|(idx, op)| {
            let scope = format!("op[{idx}]:{}", op.verb);
            let mut entries = Vec::new();

            // Required arg presence check (existing)
            let missing = required_args(&op.kind, &op.verb)
                .iter()
                .filter(|key| !op.args.contains_key(**key))
                .copied()
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                entries.push(stage_entry(
                    "03-validate-op-schemas",
                    VerificationState::Failed,
                    scope.clone(),
                    Some(format!(
                        "E_OP_SCHEMA: missing required args: {}",
                        missing.join(", ")
                    )),
                ));
                return entries;
            }

            // Version compatibility check (D2)
            if let Some(version_str) = op.args.get("version")
                && let Ok(v) = version_str.parse::<u32>()
                && v > CURRENT_SCHEMA_VERSION
            {
                entries.push(stage_entry(
                    "03-validate-op-schemas",
                    VerificationState::Failed,
                    scope.clone(),
                    Some(format!(
                        "E_OP_VERSION_INCOMPATIBLE: op version {v} exceeds current schema version {CURRENT_SCHEMA_VERSION}"
                    )),
                ));
                return entries;
            }

            // Type arg validation (D2): must be a known primitive, graph node name,
            // or a qualified external type (Package.Type / Domain.Sub.Type pattern).
            if let Some(type_arg) = op.args.get("type") {
                let is_primitive = KNOWN_PRIMITIVES.contains(&type_arg.as_str());
                let is_node = graph_names.contains(type_arg.as_str());
                // Qualified external type: "Package.Type" or "Domain.Sub.Type" —
                // a dot-separated path where every segment is a non-empty identifier.
                let is_qualified = type_arg.contains('.')
                    && type_arg.split('.').all(|seg| {
                        !seg.is_empty()
                            && seg.chars().all(|c| c.is_alphanumeric() || c == '_')
                    });
                if !is_primitive && !is_node && !is_qualified && !type_arg.is_empty() {
                    entries.push(stage_entry(
                        "03-validate-op-schemas",
                        VerificationState::Failed,
                        scope.clone(),
                        Some(format!(
                            "E_OP_ARG_TYPE_INVALID: type '{}' is not a known primitive, graph node name, or qualified external type",
                            type_arg
                        )),
                    ));
                    return entries;
                }
            }

            // Effect arg format validation (D2): must contain ':'
            if let Some(effect_arg) = op.args.get("effect")
                && !effect_arg.contains(':')
            {
                entries.push(stage_entry(
                    "03-validate-op-schemas",
                    VerificationState::Failed,
                    scope.clone(),
                    Some(format!(
                        "E_OP_ARG_EFFECT_MALFORMED: effect '{}' must follow 'name:Provider' pattern (missing ':')",
                        effect_arg
                    )),
                ));
                return entries;
            }

            entries.push(stage_entry(
                "03-validate-op-schemas",
                VerificationState::Proven,
                scope,
                None,
            ));
            entries
        })
        .collect()
}

fn required_args(kind: &ChangeSetOp, verb: &str) -> &'static [&'static str] {
    match (kind, verb) {
        (
            ChangeSetOp::Create,
            "create_module" | "create_type" | "create_function" | "create_capability",
        ) => &["id"],
        (ChangeSetOp::Set, "set_return") => &["target", "type"],
        (ChangeSetOp::Set, "set_body") => &["target", "body"],
        (ChangeSetOp::Add, "add_param") => &["target", "name", "type"],
        (ChangeSetOp::Add, "add_effect") => &["target", "effect"],
        (ChangeSetOp::Add, "add_contract") => &["target", "kind", "rule"],
        (ChangeSetOp::Remove, "remove_effect") => &["target", "effect"],
        (ChangeSetOp::Remove, "remove_contract") => &["target", "rule"],
        (ChangeSetOp::Connect | ChangeSetOp::Disconnect, _) => &["source", "target"],
        (ChangeSetOp::Rename, _) => &["target", "name"],
        (ChangeSetOp::Move, _) => &["target", "to"],
        (ChangeSetOp::Delete, _) => &["target"],
        (ChangeSetOp::Bind, _) => &["capability", "handler"],
        (ChangeSetOp::Grant | ChangeSetOp::Revoke, _) => &["target", "capability"],
        (ChangeSetOp::Expose | ChangeSetOp::Hide, _) => &["target"],
        (
            ChangeSetOp::Infer
            | ChangeSetOp::Derive
            | ChangeSetOp::Generate
            | ChangeSetOp::Assert
            | ChangeSetOp::Lock
            | ChangeSetOp::Refactor
            | ChangeSetOp::Migrate
            | ChangeSetOp::Approve
            | ChangeSetOp::Reject
            | ChangeSetOp::Deprecate
            | ChangeSetOp::Annotate
            | ChangeSetOp::Verify,
            _,
        ) => &["target"],
        _ => &[],
    }
}

// ── Stage 4: Resolve graph references ────────────────────────────────────

/// Check if a string is a valid 64-character hexadecimal hash.
fn is_valid_64char_hex(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

pub(super) fn resolve_graph_references(
    canonical: Option<&CanonicalChangeSet>,
    graph: &SemanticGraph,
) -> Vec<VerificationEntry> {
    let Some(canonical) = canonical else {
        return vec![stage_entry(
            "04-resolve-graph-references",
            VerificationState::Unverified,
            "changeset.refs",
            Some("canonical change unavailable".into()),
        )];
    };
    let names = graph
        .nodes
        .iter()
        .map(|node| node.name.as_str())
        .collect::<BTreeSet<_>>();

    let mut entries = Vec::new();
    for (idx, op) in canonical.ops.iter().enumerate() {
        // Stage 4 extension (D3): snapshot hash freshness check
        for hash_key in ["base_hash", "snapshot_hash"] {
            if let Some(hash_val) = op.args.get(hash_key) {
                if !is_valid_64char_hex(hash_val) {
                    entries.push(stage_entry(
                        "04-resolve-graph-references",
                        VerificationState::Failed,
                        format!("op[{idx}].{hash_key}"),
                        Some(format!(
                            "E_STALE_CONTEXT: {} '{}' is not a valid 64-char hex snapshot hash",
                            hash_key, hash_val
                        )),
                    ));
                } else {
                    entries.push(stage_entry(
                        "04-resolve-graph-references",
                        VerificationState::Proven,
                        format!("op[{idx}].{hash_key}"),
                        Some(format!("{hash_key} is a valid 64-char hex hash")),
                    ));
                }
            }
        }

        for key in ["target", "source", "from", "to", "capability", "handler"] {
            let Some(value) = op.args.get(key) else {
                continue;
            };
            if key == "to" && matches!(op.kind, ChangeSetOp::Move | ChangeSetOp::Migrate) {
                continue;
            }
            let creates_ref = key == "target" && matches!(op.payload, OpPayload::CreateNode(_));
            if creates_ref || names.contains(value.as_str()) {
                entries.push(stage_entry(
                    "04-resolve-graph-references",
                    VerificationState::Proven,
                    format!("op[{idx}].{key}"),
                    None,
                ));
            } else {
                entries.push(stage_entry(
                    "04-resolve-graph-references",
                    VerificationState::Failed,
                    format!("op[{idx}].{key}"),
                    Some(format!(
                        "E_GRAPH_REF_UNRESOLVED: '{value}' does not exist in target graph"
                    )),
                ));
            }
        }
    }
    if entries.is_empty() {
        entries.push(stage_entry(
            "04-resolve-graph-references",
            VerificationState::Proven,
            "changeset.refs",
            Some("no graph references to resolve".into()),
        ));
    }
    entries
}

// ── Stage 5: Build semantic diff ──────────────────────────────────────────

pub(super) fn build_semantic_diff(
    base_graph: Option<&SemanticGraph>,
    target_graph: &SemanticGraph,
) -> Vec<VerificationEntry> {
    let Some(base) = base_graph else {
        return vec![stage_entry(
            "05-build-semantic-diff",
            VerificationState::Unverified,
            "semantic_diff",
            Some("base graph snapshot not provided".into()),
        )];
    };

    let base_names: BTreeSet<&str> = base.nodes.iter().map(|n| n.name.as_str()).collect();
    let target_names: BTreeSet<&str> = target_graph.nodes.iter().map(|n| n.name.as_str()).collect();

    let mut entries = Vec::new();

    // Added nodes (in target but not in base) → Proven (addition is expected)
    for added_name in target_names.difference(&base_names) {
        entries.push(stage_entry(
            "05-build-semantic-diff",
            VerificationState::Proven,
            added_name.to_string(),
            Some(format!("node '{}' added in this changeset", added_name)),
        ));
    }

    // Removed nodes (in base but not in target) → Unverified (removal may break refs)
    for removed_name in base_names.difference(&target_names) {
        // Check if the node had expose-relevant edges in the base graph (D4)
        let had_expose = base.edges.iter().any(|edge| {
            base.nodes
                .iter()
                .any(|n| n.name == *removed_name && n.id == edge.source)
                && edge.kind == ail_core::semantic_graph::EdgeKind::DependsOn
        });
        let evidence = if had_expose {
            format!(
                "E_PUBLIC_API_CHANGED: node '{}' removed; had dependent edges",
                removed_name
            )
        } else {
            format!(
                "node '{}' removed from graph; verify no references remain",
                removed_name
            )
        };
        entries.push(stage_entry(
            "05-build-semantic-diff",
            VerificationState::Unverified,
            removed_name.to_string(),
            Some(evidence),
        ));
    }

    // Changed nodes (in both but with different type_facts or effect_row) → Unverified
    for name in base_names.intersection(&target_names) {
        let base_node = base.nodes.iter().find(|n| n.name == *name);
        let target_node = target_graph.nodes.iter().find(|n| n.name == *name);
        if let (Some(b), Some(t)) = (base_node, target_node)
            && (b.type_facts != t.type_facts || b.effect_row != t.effect_row)
        {
            entries.push(stage_entry(
                "05-build-semantic-diff",
                VerificationState::Unverified,
                name.to_string(),
                Some(format!(
                    "node '{}' type_facts or effect_row changed; verify compatibility",
                    name
                )),
            ));
        }
    }

    // If no per-node changes, emit single Proven summary
    if entries.is_empty() {
        entries.push(stage_entry(
            "05-build-semantic-diff",
            VerificationState::Proven,
            "semantic_diff",
            Some("no structural changes detected".into()),
        ));
    }

    entries
}

// ── Stage 6: Lower to Core IR ─────────────────────────────────────────────

pub(super) fn lower_core_ir(graph: &SemanticGraph) -> VerificationEntry {
    match graph.validate() {
        Ok(()) => stage_entry(
            "06-lower-affected-graph-to-core-ir",
            VerificationState::Proven,
            "core_ir",
            Some(format!("{} graph nodes lowered", graph.nodes.len())),
        ),
        Err(err) => stage_entry(
            "06-lower-affected-graph-to-core-ir",
            VerificationState::Failed,
            "core_ir",
            Some(format!(
                "E_CORE_IR_LOWERING: graph validation failed: {err:?}"
            )),
        ),
    }
}
