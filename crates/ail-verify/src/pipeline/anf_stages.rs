// ── ail-verify::pipeline::anf_stages ─────────────────────────────────────
//
// Stages 18–21 helpers: approval record validation, ANF lowering, ANF
// effect/resource ordering, and capability manifest validation.
//
// All functions are pure, behavior-preserving extractions from the original
// `pipeline.rs` godfile.  Called by `VerificationPipeline::run_with_changeset`
// in the parent module.

use std::collections::BTreeSet;

use ail_core::semantic_graph::SemanticGraph;

use crate::policy::ApprovalRecord;
use crate::report::{VerificationEntry, VerificationState};

use super::stage_entry;

// ── Stage 18: Check approval records ─────────────────────────────────────

pub(super) fn check_approval_records(approvals: &[ApprovalRecord]) -> Vec<VerificationEntry> {
    if approvals.is_empty() {
        return vec![stage_entry(
            "18-check-approval-records",
            VerificationState::Proven,
            "approvals",
            Some("no approval records required by input".into()),
        )];
    }
    approvals
        .iter()
        .map(|approval| {
            let valid = !approval.scope.trim().is_empty()
                && !approval.approver.trim().is_empty()
                && !approval.reason.trim().is_empty();
            stage_entry(
                "18-check-approval-records",
                if valid {
                    VerificationState::Proven
                } else {
                    VerificationState::Failed
                },
                approval.scope.clone(),
                if valid {
                    None
                } else {
                    Some("E_APPROVAL_RECORD_INCOMPLETE".into())
                },
            )
        })
        .collect()
}

// ── Stage 19: Lower to ANF ────────────────────────────────────────────────

/// Returns true if the body contains a `let ... in` pattern (valid ANF structure).
fn has_let_in_pattern(body: &str) -> bool {
    if let Some(let_pos) = body.find("let ") {
        body[let_pos..].contains(" in ")
    } else {
        false
    }
}

pub(super) fn lower_anf(graph: &SemanticGraph) -> Vec<VerificationEntry> {
    use ail_core::semantic_graph::NodeKind;
    let mut entries = Vec::new();

    // Body-less function check: Function nodes with no body_expr cannot be
    // lowered to ANF.  Each such node gets a structured diagnostic entry so
    // callers can programmatically distinguish missing implementations from
    // verified ones.  The error code `E_ANF_NO_BODY` is machine-parseable.
    for node in &graph.nodes {
        if node.kind == NodeKind::Function && node.body_expr.is_none() {
            entries.push(stage_entry(
                "19-lower-to-anf",
                VerificationState::Unverified,
                node.name.clone(),
                Some(format!(
                    "E_ANF_NO_BODY: function '{}' has no body expression",
                    node.name
                )),
            ));
        }
    }

    // Structural ANF check: detect bodies with non-ANF control flow.
    let violation = graph
        .nodes
        .iter()
        .filter_map(|node| node.body_expr.as_ref().map(|body| (node, body)))
        .find(|(_, body)| {
            // Non-ANF: imperative control flow keywords
            if body.contains("while ") || body.contains("for ") || body.contains("loop ") {
                return true;
            }
            // Non-ANF: bare semicolons outside of a let...in context
            if body.contains(';') && !has_let_in_pattern(body) {
                return true;
            }
            false
        });
    if let Some((node, body)) = violation {
        entries.push(stage_entry(
            "19-lower-to-anf",
            VerificationState::Unverified,
            node.name.clone(),
            Some(format!("body requires non-trivial ANF lowering: {body}")),
        ));
        return entries;
    }

    // Summary Proven entry when no structural violations were found.
    entries.push(stage_entry(
        "19-lower-to-anf",
        VerificationState::Proven,
        "anf_ir",
        Some("graph expressions are ANF-compatible".into()),
    ));
    entries
}

// ── Stage 20: Check ANF effect/resource ordering ──────────────────────────

/// Scan `body` for `acquire(<ident>)` and `release(<ident>)` tokens.
/// Returns the identifier name where a release appears before the corresponding
/// acquire (or release without any acquire).
fn find_ordering_violation(body: &str) -> Option<String> {
    use std::collections::HashMap;

    let mut acquires: HashMap<String, usize> = HashMap::new();
    let mut releases: HashMap<String, usize> = HashMap::new();

    // Walk the body scanning for acquire(<ident>) and release(<ident>)
    let mut pos = 0;
    while pos < body.len() {
        for (keyword, map) in [("acquire(", &mut acquires), ("release(", &mut releases)] {
            if body[pos..].starts_with(keyword) {
                let inner_start = pos + keyword.len();
                if let Some(close) = body[inner_start..].find(')') {
                    let ident = body[inner_start..inner_start + close].trim().to_string();
                    if !ident.is_empty()
                        && ident
                            .chars()
                            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
                    {
                        map.entry(ident).or_insert(pos);
                    }
                }
            }
        }
        pos += 1;
    }

    // Check: any release appearing before its corresponding acquire
    for (ident, release_pos) in &releases {
        if let Some(&acquire_pos) = acquires.get(ident) {
            if *release_pos < acquire_pos {
                return Some(ident.clone());
            }
        } else {
            // release without matching acquire is also a violation
            return Some(ident.clone());
        }
    }
    None
}

/// Scan `body` for effect ordering violations:
/// - `run_effect(ident)` before `bind_effect(ident)` → E_ANF_EFFECT_ORDER
/// - `run_effect(ident)` without any `bind_effect(ident)` → E_ANF_EFFECT_ORDER
/// - `emit_effect(ident)` appearing more than once → E_ANF_DUPLICATE_EFFECT
fn find_effect_ordering_violation(body: &str) -> Option<String> {
    use std::collections::HashMap;

    let mut binds: HashMap<String, usize> = HashMap::new();
    let mut runs: HashMap<String, usize> = HashMap::new();
    let mut emits: HashMap<String, usize> = HashMap::new();

    // Walk the body scanning for effect keywords.
    let mut pos = 0;
    while pos < body.len() {
        for (keyword, map) in [
            ("bind_effect(", &mut binds),
            ("run_effect(", &mut runs),
            ("emit_effect(", &mut emits),
        ] {
            if body[pos..].starts_with(keyword) {
                let inner_start = pos + keyword.len();
                if let Some(close) = body[inner_start..].find(')') {
                    let ident = body[inner_start..inner_start + close].trim().to_string();
                    if !ident.is_empty()
                        && ident
                            .chars()
                            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
                    {
                        *map.entry(ident).or_insert(0) += 1;
                    }
                }
            }
        }
        pos += 1;
    }

    // Check: emit_effect duplicate
    for (ident, count) in &emits {
        if *count > 1 {
            return Some(format!("E_ANF_DUPLICATE_EFFECT:{ident}"));
        }
    }

    // Check: run_effect before bind_effect (positional scan)
    for ident in runs.keys() {
        let bind_pos = find_keyword_pos(body, "bind_effect(", ident);
        let run_pos = find_keyword_pos(body, "run_effect(", ident);
        match (bind_pos, run_pos) {
            (None, Some(_)) => return Some(format!("E_ANF_EFFECT_ORDER_NO_BIND:{ident}")),
            (Some(b), Some(r)) if r < b => {
                return Some(format!("E_ANF_EFFECT_ORDER:{ident}"));
            }
            _ => {}
        }
    }
    None
}

/// Find the byte position of the first `keyword + ident + ")"` in `body`.
fn find_keyword_pos(body: &str, keyword: &str, ident: &str) -> Option<usize> {
    let mut pos = 0;
    while pos < body.len() {
        if body[pos..].starts_with(keyword) {
            let inner_start = pos + keyword.len();
            if let Some(close) = body[inner_start..].find(')') {
                let found = body[inner_start..inner_start + close].trim();
                if found == ident {
                    return Some(pos);
                }
            }
        }
        pos += 1;
    }
    None
}

pub(super) fn check_anf_ordering(graph: &SemanticGraph) -> VerificationEntry {
    for node in &graph.nodes {
        let Some(body) = &node.body_expr else {
            continue;
        };
        // Resource ordering check (existing — runs first, ANF-4)
        if let Some(ident) = find_ordering_violation(body) {
            return stage_entry(
                "20-check-anf-effect-resource-ordering",
                VerificationState::Failed,
                node.name.clone(),
                Some(format!(
                    "E_ANF_RESOURCE_ORDER: release('{}') appears before acquire('{}')",
                    ident, ident
                )),
            );
        }
        // Effect ordering check (new)
        if let Some(violation) = find_effect_ordering_violation(body) {
            let (code, detail) =
                if let Some(ident) = violation.strip_prefix("E_ANF_DUPLICATE_EFFECT:") {
                    (
                        "E_ANF_DUPLICATE_EFFECT",
                        format!("emit_effect('{ident}') appears more than once"),
                    )
                } else if let Some(ident) = violation.strip_prefix("E_ANF_EFFECT_ORDER_NO_BIND:") {
                    (
                        "E_ANF_EFFECT_ORDER",
                        format!("run_effect('{ident}') without bind_effect"),
                    )
                } else if let Some(ident) = violation.strip_prefix("E_ANF_EFFECT_ORDER:") {
                    (
                        "E_ANF_EFFECT_ORDER",
                        format!("run_effect('{ident}') before bind_effect('{ident}')"),
                    )
                } else {
                    ("E_ANF_EFFECT_ORDER", violation)
                };
            return stage_entry(
                "20-check-anf-effect-resource-ordering",
                VerificationState::Failed,
                node.name.clone(),
                Some(format!("{code}: {detail}")),
            );
        }
    }
    stage_entry(
        "20-check-anf-effect-resource-ordering",
        VerificationState::Proven,
        "anf_ir",
        Some("effect/resource ordering preserved".into()),
    )
}

// ── Stage 21: Generate/validate manifest ─────────────────────────────────

/// Compute a deterministic 64-char hex hash from sorted capability names.
///
/// Uses a FNV-64-inspired accumulation expanded to 256 bits (4 × u64) so the
/// output is a valid 64-character hex string compatible with Stage 21 hash
/// comparison.  The computation is pure and produces identical output for
/// identical inputs across all platforms.
fn compute_caps_hash(cap_names: &[&str]) -> String {
    let mut sorted: Vec<&str> = cap_names.to_vec();
    sorted.sort();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for name in &sorted {
        for byte in name.bytes() {
            h ^= byte as u64;
            h = h.wrapping_mul(0x0000_0001_0000_01b3);
        }
        h = h.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
    let h1 = h;
    let h2 = h
        .wrapping_mul(0x517c_c1b7_2722_0a95)
        .wrapping_add(0xf6bd_bff8_bce2_4095);
    let h3 = h
        .wrapping_mul(0xbf58_476d_1ce4_e5b9)
        .wrapping_add(0x94d0_49bb_1331_11eb);
    let h4 = h
        .wrapping_mul(0x6c62_272e_07bb_0142)
        .wrapping_add(0x62b8_2175_6295_c58d);
    format!("{h1:016x}{h2:016x}{h3:016x}{h4:016x}")
}

pub(super) fn validate_manifest(
    graph: &SemanticGraph,
    manifest_caps: &[String],
    artifact_manifest_hash: Option<&str>,
) -> VerificationEntry {
    let graph_caps = graph
        .nodes
        .iter()
        .filter(|node| node.kind == ail_core::semantic_graph::NodeKind::Capability)
        .map(|node| node.name.as_str())
        .collect::<BTreeSet<_>>();

    // Hash comparison (Ola5 Gap-2): when artifact_manifest_hash is provided,
    // compute the actual capability-set hash and compare it first.
    if let Some(expected_hash) = artifact_manifest_hash {
        let cap_names: Vec<&str> = graph_caps.iter().copied().collect();
        let actual_hash = compute_caps_hash(&cap_names);
        if actual_hash != expected_hash {
            return stage_entry(
                "21-generate-validate-manifest",
                VerificationState::Failed,
                "capabilities_manifest",
                Some(format!(
                    "E_MANIFEST_HASH_MISMATCH: expected {expected_hash}, computed {actual_hash}"
                )),
            );
        }
    }

    if manifest_caps.is_empty() && graph_caps.is_empty() {
        return stage_entry(
            "21-generate-validate-manifest",
            VerificationState::Proven,
            "capabilities_manifest",
            Some("no capabilities required".into()),
        );
    }
    let manifest = manifest_caps
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if graph_caps == manifest {
        stage_entry(
            "21-generate-validate-manifest",
            VerificationState::Proven,
            "capabilities_manifest",
            Some(format!("{} capabilities validated", graph_caps.len())),
        )
    } else {
        stage_entry(
            "21-generate-validate-manifest",
            VerificationState::Failed,
            "capabilities_manifest",
            Some(
                "E_MANIFEST_MISMATCH: graph capabilities differ from manifest capabilities".into(),
            ),
        )
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};

    use crate::report::VerificationState;

    use super::check_anf_ordering;

    fn graph_with_body(body: &str) -> SemanticGraph {
        let mut node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.test");
        node.body_expr = Some(body.to_string());
        SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        }
    }

    // ── T-09 / T-10: ANF effect ordering ─────────────────────────────────

    #[test]
    fn anf_run_before_bind_fails() {
        let graph = graph_with_body("run_effect(db); bind_effect(db)");
        let entry = check_anf_ordering(&graph);
        assert_eq!(entry.state, VerificationState::Failed);
        assert!(
            entry
                .evidence
                .as_deref()
                .unwrap_or("")
                .contains("E_ANF_EFFECT_ORDER"),
            "evidence must contain E_ANF_EFFECT_ORDER"
        );
    }

    #[test]
    fn anf_run_without_bind_fails() {
        let graph = graph_with_body("run_effect(db)");
        let entry = check_anf_ordering(&graph);
        assert_eq!(entry.state, VerificationState::Failed);
        assert!(
            entry
                .evidence
                .as_deref()
                .unwrap_or("")
                .contains("E_ANF_EFFECT_ORDER"),
            "run_effect without bind_effect must produce E_ANF_EFFECT_ORDER"
        );
    }

    #[test]
    fn anf_duplicate_emit_fails() {
        let graph = graph_with_body("emit_effect(log); emit_effect(log)");
        let entry = check_anf_ordering(&graph);
        assert_eq!(entry.state, VerificationState::Failed);
        assert!(
            entry
                .evidence
                .as_deref()
                .unwrap_or("")
                .contains("E_ANF_DUPLICATE_EFFECT"),
            "duplicate emit_effect must produce E_ANF_DUPLICATE_EFFECT"
        );
    }

    #[test]
    fn anf_valid_bind_then_run_passes() {
        let graph = graph_with_body("bind_effect(db); run_effect(db)");
        let entry = check_anf_ordering(&graph);
        assert_eq!(entry.state, VerificationState::Proven);
    }

    #[test]
    fn anf_valid_single_emit_passes() {
        let graph = graph_with_body("emit_effect(log)");
        let entry = check_anf_ordering(&graph);
        assert_eq!(entry.state, VerificationState::Proven);
    }

    // Existing resource ordering must still work (ANF-3)
    #[test]
    fn anf_release_before_acquire_still_fails() {
        let graph = graph_with_body("release(conn); acquire(conn)");
        let entry = check_anf_ordering(&graph);
        assert_eq!(entry.state, VerificationState::Failed);
        assert!(
            entry
                .evidence
                .as_deref()
                .unwrap_or("")
                .contains("E_ANF_RESOURCE_ORDER"),
            "resource order violation must produce E_ANF_RESOURCE_ORDER"
        );
    }

    // ── Body-less function structured diagnostics ─────────────────────────

    // RED → GREEN: A Function node with no body_expr produces a structured
    // E_ANF_NO_BODY diagnostic in stage 19-lower-to-anf.
    #[test]
    fn bodyless_function_produces_e_anf_no_body_diagnostic() {
        use super::lower_anf;

        let node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn_stub");
        // No body_expr set — this is the body-less case.
        let graph = SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        };
        let entries = lower_anf(&graph);
        // Must include exactly one entry for the body-less function.
        let stub_entry = entries
            .iter()
            .find(|e| e.scope == "fn_stub")
            .expect("must have an entry for fn_stub");
        assert_eq!(
            stub_entry.state,
            VerificationState::Unverified,
            "body-less function must be Unverified, not Proven or Failed"
        );
        assert!(
            stub_entry
                .evidence
                .as_deref()
                .unwrap_or("")
                .starts_with("E_ANF_NO_BODY"),
            "evidence must start with structured code E_ANF_NO_BODY, got: {:?}",
            stub_entry.evidence
        );
        assert!(
            stub_entry
                .evidence
                .as_deref()
                .unwrap_or("")
                .contains("fn_stub"),
            "evidence must include the function name"
        );
    }

    // TRIANGULATE: Multiple body-less functions each get their own E_ANF_NO_BODY entry.
    #[test]
    fn multiple_bodyless_functions_each_get_e_anf_no_body_entry() {
        use super::lower_anf;

        let graph = SemanticGraph {
            nodes: vec![
                GraphNode::new(NodeRef(0), NodeKind::Function, "fn_a"),
                GraphNode::new(NodeRef(1), NodeKind::Function, "fn_b"),
            ],
            edges: vec![],
        };
        let entries = lower_anf(&graph);
        let no_body_entries: Vec<_> = entries
            .iter()
            .filter(|e| {
                e.evidence
                    .as_deref()
                    .unwrap_or("")
                    .starts_with("E_ANF_NO_BODY")
            })
            .collect();
        assert_eq!(
            no_body_entries.len(),
            2,
            "each body-less function must have its own E_ANF_NO_BODY entry"
        );
    }

    // TRIANGULATE: A Function WITH a body does not produce E_ANF_NO_BODY.
    #[test]
    fn function_with_body_does_not_produce_e_anf_no_body() {
        use super::lower_anf;

        let graph = graph_with_body("let x = 1 in x");
        let entries = lower_anf(&graph);
        let has_no_body = entries.iter().any(|e| {
            e.evidence
                .as_deref()
                .unwrap_or("")
                .starts_with("E_ANF_NO_BODY")
        });
        assert!(
            !has_no_body,
            "a function with a body must not produce E_ANF_NO_BODY"
        );
    }
}
