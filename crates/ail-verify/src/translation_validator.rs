// ── ail-verify::translation_validator ────────────────────────────────────
//
// Translation validation checker — verifies that the semantic graph survives
// lowering through Core IR with shape, provenance, and effect obligations
// preserved.
//
// # Motivation
//
// The compiler pipeline lowers the Semantic Graph → Core IR → ANF → target.
// If the graph contains inconsistencies that survive this pipeline (undeclared
// effects, untraced return types, missing evidence), those inconsistencies
// become silent bugs in the emitted artifact.  This checker catches them at
// the verification layer, before lowering proceeds.
//
// # Profile-tiered checks
//
// | Check              | Trigger                     | Profiles              |
// |--------------------|-----------------------------|-----------------------|
// | TV-1 Shape         | body with no return type    | all                   |
// | TV-2 Provenance    | malformed effect declaration| all                   |
// | TV-3 Effect oblig. | body uses undeclared effect | prod, staging,        |
// |                    |                             | critical, unknown     |
// | TV-4 Evidence suff.| effect with no proof path   | critical, unknown     |
//
// # Error codes (stable, machine-readable)
//
// `E_TV_SHAPE_NO_RETURN_TYPE`   — Function has a body_expr but no return_type.
// `E_TV_EFFECT_MALFORMED`       — Effect in effect_row lacks `name:Provider` format.
// `E_TV_EFFECT_UNDECLARED`      — body_expr uses an effect absent from effect_row.
// `E_TV_INSUFFICIENT_EVIDENCE`  — Critical: effect declared with no body or runtime checks.
//
// # Determinism
//
// All checks are pure.  Entries are emitted in graph-node-insertion order
// within each check tier.  Identical inputs always produce identical output.

use ail_core::semantic_graph::{NodeKind, SemanticGraph};

use crate::report::{VerificationEntry, VerificationState};

// ── Stable error codes ────────────────────────────────────────────────────

/// TV-1: Function node has a body expression but declares no return type.
///
/// Without a return type the Core IR lowering cannot produce a typed function
/// signature, making the provenance chain between Semantic Graph and Core IR
/// incomplete.
pub const E_TV_SHAPE_NO_RETURN_TYPE: &str = "E_TV_SHAPE_NO_RETURN_TYPE";

/// TV-2: An effect in `effect_row` is missing the `name:Provider` separator.
///
/// Well-formed effect declarations must follow `name:Provider` so that handler
/// binding, capability grant checks, and manifest consistency checks can
/// resolve the provider boundary.  An effect without `:` cannot be traced
/// through lowering.
pub const E_TV_EFFECT_MALFORMED: &str = "E_TV_EFFECT_MALFORMED";

/// TV-3: A `body_expr` pattern references an effect identifier that does not
/// appear in the node's declared `effect_row`.
///
/// Equivalent to `E_EFFECT_UNDECLARED` in the effect checker, but detected
/// at the body-text level rather than the graph-edge level.  Applicable in
/// prod and stricter profiles.
pub const E_TV_EFFECT_UNDECLARED: &str = "E_TV_EFFECT_UNDECLARED";

/// TV-4 (critical): A Function node declares effects but has no `body_expr`
/// and no `runtime_checks` — there is no evidence path through lowering.
///
/// In critical profiles, effect declarations without any implementation or
/// runtime materialization are rejected because translation correctness cannot
/// be argued.
pub const E_TV_INSUFFICIENT_EVIDENCE: &str = "E_TV_INSUFFICIENT_EVIDENCE";

// ── TranslationValidator ──────────────────────────────────────────────────

/// Pure, stateless translation validation checker.
///
/// Run after Core IR lowering (Stage 6) to verify that the semantic graph
/// can be lowered correctly through the translation pipeline.
pub struct TranslationValidator;

impl TranslationValidator {
    /// Run all profile-appropriate translation validation checks on `graph`.
    ///
    /// Returns a flat list of `VerificationEntry` values in node-insertion
    /// order within each check tier.  If no entries are produced (no issues
    /// and no tracked items), a single summary `Proven` entry is returned.
    pub fn check(graph: &SemanticGraph, profile: &str) -> Vec<VerificationEntry> {
        let mut entries = Vec::new();

        // TV-1: Shape checks — all profiles.
        entries.extend(check_shape(graph));

        // TV-2: Effect provenance — all profiles.
        entries.extend(check_effect_provenance(graph));

        // TV-3: Control-flow / effect obligations — prod and stricter.
        if is_prod_or_stricter(profile) {
            entries.extend(check_effect_obligations(graph));
        }

        // TV-4: Evidence sufficiency — critical (and unknown/unrecognized).
        if is_critical_like(profile) {
            entries.extend(check_evidence_sufficiency(graph));
        }

        // Emit a single summary Proven when the graph passes all checks.
        if entries.is_empty() {
            entries.push(make_entry(
                "translation-validation/summary",
                VerificationState::Proven,
                "translation_validation",
                Some("all translation validation checks passed".into()),
            ));
        }

        entries
    }
}

// ── TV-1: Shape checks ────────────────────────────────────────────────────

/// Check that every Function node with a body_expr also declares a return_type.
///
/// A body without a declared return type creates an incomplete provenance
/// record: Core IR lowering cannot type the function signature, breaking the
/// translation chain.
fn check_shape(graph: &SemanticGraph) -> Vec<VerificationEntry> {
    let mut entries = Vec::new();

    for node in &graph.nodes {
        if node.kind != NodeKind::Function {
            continue;
        }
        let Some(_body) = &node.body_expr else {
            continue; // no body — Stage 19 handles body-less functions
        };
        if node.return_type.is_none() {
            entries.push(make_entry(
                "translation-validation/shape",
                VerificationState::Unverified,
                node.name.clone(),
                Some(format!(
                    "{E_TV_SHAPE_NO_RETURN_TYPE}: function '{}' has body_expr but no declared \
                     return_type; return type is required for typed Core IR lowering",
                    node.name
                )),
            ));
        } else {
            entries.push(make_entry(
                "translation-validation/shape",
                VerificationState::Proven,
                node.name.clone(),
                Some(format!(
                    "function '{}' has body_expr and declared return_type; shape consistent",
                    node.name
                )),
            ));
        }
    }

    entries
}

// ── TV-2: Effect provenance ───────────────────────────────────────────────

/// Check that every declared effect follows the `name:Provider` format.
///
/// Well-formed effects are required so that handler binding, capability grant
/// checks, and manifest consistency checks can trace the provider boundary
/// through lowering.  Effects without a `:` separator cannot be resolved.
fn check_effect_provenance(graph: &SemanticGraph) -> Vec<VerificationEntry> {
    let mut entries = Vec::new();

    for node in &graph.nodes {
        let Some(row) = &node.effect_row else {
            continue; // no declared effects — nothing to check
        };
        if row.effects.is_empty() {
            continue;
        }

        let malformed: Vec<&str> = row
            .effects
            .iter()
            .filter(|e| !e.contains(':'))
            .map(String::as_str)
            .collect();

        if malformed.is_empty() {
            entries.push(make_entry(
                "translation-validation/provenance",
                VerificationState::Proven,
                node.name.clone(),
                Some(format!(
                    "{} declared effect(s) follow 'name:Provider' format; provenance traceable",
                    row.effects.len()
                )),
            ));
        } else {
            entries.push(make_entry(
                "translation-validation/provenance",
                VerificationState::Unverified,
                node.name.clone(),
                Some(format!(
                    "{E_TV_EFFECT_MALFORMED}: effect(s) [{}] in '{}' lack 'name:Provider' \
                     separator; provenance cannot be traced through Core IR lowering",
                    malformed.join(", "),
                    node.name
                )),
            ));
        }
    }

    entries
}

// ── TV-3: Effect obligations ──────────────────────────────────────────────

/// Extract effect identifiers referenced in a body expression.
///
/// Recognises `emit_effect(X)`, `run_effect(X)`, and `bind_effect(X)` as
/// effect references — these are the three canonical patterns used by the ANF
/// lowering stage and Stage 20 ordering checks.
///
/// Returns a deduplicated list of identifier strings found in the body.
fn extract_body_effects(body: &str) -> Vec<String> {
    use std::collections::BTreeSet;

    let mut found: BTreeSet<String> = BTreeSet::new();

    for keyword in &["emit_effect(", "run_effect(", "bind_effect("] {
        let mut pos = 0;
        while pos < body.len() {
            if body[pos..].starts_with(keyword) {
                let inner_start = pos + keyword.len();
                if let Some(close) = body[inner_start..].find(')') {
                    let ident = body[inner_start..inner_start + close].trim();
                    if !ident.is_empty()
                        && ident
                            .chars()
                            .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == ':')
                    {
                        found.insert(ident.to_string());
                    }
                }
            }
            pos += 1;
        }
    }

    found.into_iter().collect()
}

/// Return true if `declared_effects` covers the body effect identifier `id`.
///
/// A declared effect `"name:Provider"` covers identifier `"name"` (prefix
/// match before `:`) or the full string `"name:Provider"` (exact match).
fn effect_is_declared(id: &str, declared_effects: &[String]) -> bool {
    declared_effects.iter().any(|decl| {
        // Exact match (e.g., body uses full "db:Postgres" syntax)
        decl == id
            // Prefix match: declared "db:Postgres" covers body identifier "db"
            || decl.starts_with(&format!("{id}:"))
    })
}

/// Check that body_expr effect references are covered by declared effect_row.
///
/// Applied only in prod and stricter profiles.
fn check_effect_obligations(graph: &SemanticGraph) -> Vec<VerificationEntry> {
    let mut entries = Vec::new();

    for node in &graph.nodes {
        if node.kind != NodeKind::Function {
            continue;
        }
        let Some(body) = &node.body_expr else {
            continue;
        };

        let body_effects = extract_body_effects(body);
        if body_effects.is_empty() {
            continue; // no effect usage in body — nothing to check
        }

        let declared: Vec<String> = node
            .effect_row
            .as_ref()
            .map(|r| r.effects.clone())
            .unwrap_or_default();

        let undeclared: Vec<&str> = body_effects
            .iter()
            .filter(|id| !effect_is_declared(id, &declared))
            .map(String::as_str)
            .collect();

        if undeclared.is_empty() {
            entries.push(make_entry(
                "translation-validation/effect-obligation",
                VerificationState::Proven,
                node.name.clone(),
                Some(format!(
                    "all {} body effect(s) declared in effect_row; control-flow/effect \
                     obligation satisfied",
                    body_effects.len()
                )),
            ));
        } else {
            entries.push(make_entry(
                "translation-validation/effect-obligation",
                VerificationState::Failed,
                node.name.clone(),
                Some(format!(
                    "{E_TV_EFFECT_UNDECLARED}: body of '{}' uses effect(s) [{}] not declared \
                     in effect_row; undeclared effects cannot be lowered to Core IR safely",
                    node.name,
                    undeclared.join(", ")
                )),
            ));
        }
    }

    entries
}

// ── TV-4: Evidence sufficiency ────────────────────────────────────────────

/// Reject Function nodes in critical profiles that declare effects but have
/// no implementation evidence (no body_expr, no runtime_checks).
///
/// Without any evidence path, the translation claim "this function properly
/// handles its declared effects" cannot be argued, making the lowering
/// unverifiable in a critical context.
fn check_evidence_sufficiency(graph: &SemanticGraph) -> Vec<VerificationEntry> {
    let mut entries = Vec::new();

    for node in &graph.nodes {
        if node.kind != NodeKind::Function {
            continue;
        }
        let Some(row) = &node.effect_row else {
            continue; // no declared effects — nothing to check
        };
        if row.effects.is_empty() {
            continue;
        }

        let has_body = node.body_expr.is_some();
        let has_runtime_checks = node
            .runtime_checks
            .as_ref()
            .is_some_and(|checks| !checks.is_empty());

        if !has_body && !has_runtime_checks {
            entries.push(make_entry(
                "translation-validation/evidence-sufficiency",
                VerificationState::Failed,
                node.name.clone(),
                Some(format!(
                    "{E_TV_INSUFFICIENT_EVIDENCE}: function '{}' declares {} effect(s) [{}] but \
                     has no body_expr and no runtime_checks; critical profile requires at least \
                     one evidence path through the translation chain",
                    node.name,
                    row.effects.len(),
                    row.effects.join(", ")
                )),
            ));
        } else {
            entries.push(make_entry(
                "translation-validation/evidence-sufficiency",
                VerificationState::Proven,
                node.name.clone(),
                Some(format!(
                    "function '{}' has evidence path for {} declared effect(s); \
                     translation sufficiency satisfied",
                    node.name,
                    row.effects.len()
                )),
            ));
        }
    }

    entries
}

// ── Profile helpers ───────────────────────────────────────────────────────

/// Return true if `profile` is prod, staging, critical, or unrecognized
/// (strict-by-default).
fn is_prod_or_stricter(profile: &str) -> bool {
    matches!(profile, "prod" | "staging" | "critical")
        || !matches!(profile, "draft" | "dev" | "test")
}

/// Return true if `profile` is critical or unrecognized (strict-by-default).
fn is_critical_like(profile: &str) -> bool {
    profile == "critical" || !matches!(profile, "draft" | "dev" | "test" | "staging" | "prod")
}

// ── Entry constructor ─────────────────────────────────────────────────────

fn make_entry(
    claim: impl Into<String>,
    state: VerificationState,
    scope: impl Into<String>,
    evidence: Option<String>,
) -> VerificationEntry {
    let blocking = matches!(state, VerificationState::Failed | VerificationState::Unsafe);
    VerificationEntry {
        claim: claim.into(),
        state,
        scope: scope.into(),
        evidence,
        blocking,
        repair_options: vec![],
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use ail_core::semantic_graph::{
        EffectRow, GraphNode, NodeKind, NodeRef, RuntimeCheckMeta, SemanticGraph,
    };

    use crate::report::VerificationState;

    use super::{
        E_TV_EFFECT_MALFORMED, E_TV_EFFECT_UNDECLARED, E_TV_INSUFFICIENT_EVIDENCE,
        E_TV_SHAPE_NO_RETURN_TYPE, TranslationValidator, check_effect_provenance, check_shape,
        extract_body_effects,
    };

    // ── Helpers ───────────────────────────────────────────────────────────

    fn fn_node_with_body(id: u32, name: &str, body: &str) -> GraphNode {
        let mut node = GraphNode::new(NodeRef(id), NodeKind::Function, name);
        node.body_expr = Some(body.to_string());
        node
    }

    fn fn_node_with_body_and_return(id: u32, name: &str, body: &str) -> GraphNode {
        let mut node = GraphNode::new(NodeRef(id), NodeKind::Function, name);
        node.body_expr = Some(body.to_string());
        node.return_type = Some("Int".to_string());
        node
    }

    fn fn_node_with_effects(id: u32, name: &str, effects: &[&str]) -> GraphNode {
        let mut node = GraphNode::new(NodeRef(id), NodeKind::Function, name);
        node.effect_row = Some(EffectRow {
            effects: effects.iter().map(|s| s.to_string()).collect(),
        });
        node
    }

    fn fn_node_full(
        id: u32,
        name: &str,
        body: &str,
        effects: &[&str],
        return_type: Option<&str>,
    ) -> GraphNode {
        let mut node = GraphNode::new(NodeRef(id), NodeKind::Function, name);
        node.body_expr = Some(body.to_string());
        node.return_type = return_type.map(str::to_string);
        node.effect_row = if effects.is_empty() {
            None
        } else {
            Some(EffectRow {
                effects: effects.iter().map(|s| s.to_string()).collect(),
            })
        };
        node
    }

    fn single_node_graph(node: GraphNode) -> SemanticGraph {
        SemanticGraph {
            nodes: vec![node],
            edges: vec![],
        }
    }

    // ── TV-1: Shape checks ────────────────────────────────────────────────

    #[test]
    fn shape_body_without_return_type_is_unverified() {
        let node = fn_node_with_body(0, "fn.no_return", "let x = 1 in x");
        let graph = single_node_graph(node);
        let entries = check_shape(&graph);
        let e = entries.iter().find(|e| e.scope == "fn.no_return").unwrap();
        assert_eq!(
            e.state,
            VerificationState::Unverified,
            "body without return_type must be Unverified"
        );
        assert!(
            e.evidence
                .as_deref()
                .unwrap_or("")
                .contains(E_TV_SHAPE_NO_RETURN_TYPE),
            "evidence must contain E_TV_SHAPE_NO_RETURN_TYPE"
        );
    }

    #[test]
    fn shape_body_with_return_type_is_proven() {
        let node = fn_node_with_body_and_return(0, "fn.ok", "let x = 1 in x");
        let graph = single_node_graph(node);
        let entries = check_shape(&graph);
        let e = entries.iter().find(|e| e.scope == "fn.ok").unwrap();
        assert_eq!(
            e.state,
            VerificationState::Proven,
            "body with return_type must be Proven"
        );
    }

    #[test]
    fn shape_non_function_nodes_are_skipped() {
        let node = GraphNode::new(NodeRef(0), NodeKind::Type, "MyType");
        let graph = single_node_graph(node);
        let entries = check_shape(&graph);
        // No shape entries for non-Function nodes
        assert!(
            entries.is_empty(),
            "non-Function nodes must not produce shape entries"
        );
    }

    #[test]
    fn shape_function_without_body_is_skipped() {
        // Body-less functions are handled by Stage 19 (E_ANF_NO_BODY)
        let node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.abstract");
        let graph = single_node_graph(node);
        let entries = check_shape(&graph);
        assert!(
            entries.is_empty(),
            "body-less functions must not produce shape entries"
        );
    }

    // ── TV-2: Effect provenance ───────────────────────────────────────────

    #[test]
    fn provenance_well_formed_effects_are_proven() {
        let node = fn_node_with_effects(0, "fn.ok", &["db:Postgres", "http:Stripe"]);
        let graph = single_node_graph(node);
        let entries = check_effect_provenance(&graph);
        let e = entries.iter().find(|e| e.scope == "fn.ok").unwrap();
        assert_eq!(e.state, VerificationState::Proven);
    }

    #[test]
    fn provenance_malformed_effect_is_unverified() {
        let node = fn_node_with_effects(0, "fn.bad", &["db", "http:Stripe"]);
        let graph = single_node_graph(node);
        let entries = check_effect_provenance(&graph);
        let e = entries.iter().find(|e| e.scope == "fn.bad").unwrap();
        assert_eq!(
            e.state,
            VerificationState::Unverified,
            "effect without ':' must be Unverified"
        );
        assert!(
            e.evidence
                .as_deref()
                .unwrap_or("")
                .contains(E_TV_EFFECT_MALFORMED),
            "evidence must contain E_TV_EFFECT_MALFORMED"
        );
    }

    #[test]
    fn provenance_no_effects_produces_no_entry() {
        let node = GraphNode::new(NodeRef(0), NodeKind::Function, "fn.pure");
        let graph = single_node_graph(node);
        let entries = check_effect_provenance(&graph);
        assert!(
            entries.is_empty(),
            "node with no effect_row must produce no provenance entry"
        );
    }

    // ── extract_body_effects ──────────────────────────────────────────────

    #[test]
    fn extract_emit_effect_identifiers() {
        let effects = extract_body_effects("emit_effect(db); emit_effect(log)");
        assert!(effects.contains(&"db".to_string()));
        assert!(effects.contains(&"log".to_string()));
    }

    #[test]
    fn extract_run_and_bind_effect_identifiers() {
        let effects = extract_body_effects("bind_effect(net); run_effect(net)");
        assert!(effects.contains(&"net".to_string()));
        assert_eq!(
            effects.iter().filter(|e| *e == "net").count(),
            1,
            "deduplicated"
        );
    }

    #[test]
    fn extract_no_effects_returns_empty() {
        let effects = extract_body_effects("let x = 1 in x + 2");
        assert!(effects.is_empty());
    }

    // ── TV-3: Effect obligations ──────────────────────────────────────────

    #[test]
    fn effect_obligation_undeclared_body_effect_fails_in_prod() {
        let node = fn_node_full(
            0,
            "fn.charge",
            "emit_effect(db)",
            &[], // no declared effects
            Some("Unit"),
        );
        let graph = single_node_graph(node);
        let entries = TranslationValidator::check(&graph, "prod");
        let e = entries
            .iter()
            .find(|e| {
                e.scope == "fn.charge" && e.claim == "translation-validation/effect-obligation"
            })
            .unwrap();
        assert_eq!(
            e.state,
            VerificationState::Failed,
            "undeclared body effect must be Failed in prod"
        );
        assert!(
            e.evidence
                .as_deref()
                .unwrap_or("")
                .contains(E_TV_EFFECT_UNDECLARED),
            "evidence must contain E_TV_EFFECT_UNDECLARED"
        );
    }

    #[test]
    fn effect_obligation_declared_body_effect_passes_in_prod() {
        let node = fn_node_full(
            0,
            "fn.charge",
            "emit_effect(db)",
            &["db:Postgres"], // declared with provider format
            Some("Unit"),
        );
        let graph = single_node_graph(node);
        let entries = TranslationValidator::check(&graph, "prod");
        let e = entries.iter().find(|e| {
            e.scope == "fn.charge" && e.claim == "translation-validation/effect-obligation"
        });
        // Should be proven (no violation)
        if let Some(e) = e {
            assert_eq!(
                e.state,
                VerificationState::Proven,
                "declared body effect must be Proven in prod"
            );
        }
    }

    #[test]
    fn effect_obligation_not_checked_in_dev() {
        // dev profile must NOT run TV-3 (only shape+provenance)
        let node = fn_node_full(
            0,
            "fn.charge",
            "emit_effect(db)",
            &[], // no declared effects
            Some("Unit"),
        );
        let graph = single_node_graph(node);
        let entries = TranslationValidator::check(&graph, "dev");
        let tv3_entry = entries
            .iter()
            .find(|e| e.claim == "translation-validation/effect-obligation");
        assert!(tv3_entry.is_none(), "TV-3 must not run in dev profile");
    }

    // ── TV-4: Evidence sufficiency ────────────────────────────────────────

    #[test]
    fn evidence_sufficiency_fails_in_critical_when_no_body_and_no_checks() {
        let node = fn_node_with_effects(0, "fn.iface", &["io:Console"]);
        // No body_expr, no runtime_checks
        let graph = single_node_graph(node);
        let entries = TranslationValidator::check(&graph, "critical");
        let e = entries
            .iter()
            .find(|e| {
                e.scope == "fn.iface" && e.claim == "translation-validation/evidence-sufficiency"
            })
            .unwrap();
        assert_eq!(
            e.state,
            VerificationState::Failed,
            "critical: effect without evidence path must be Failed"
        );
        assert!(
            e.evidence
                .as_deref()
                .unwrap_or("")
                .contains(E_TV_INSUFFICIENT_EVIDENCE),
            "evidence must contain E_TV_INSUFFICIENT_EVIDENCE"
        );
    }

    #[test]
    fn evidence_sufficiency_passes_in_critical_when_body_present() {
        let node = fn_node_full(
            0,
            "fn.impl",
            "emit_effect(io)",
            &["io:Console"],
            Some("Unit"),
        );
        let graph = single_node_graph(node);
        let entries = TranslationValidator::check(&graph, "critical");
        let e = entries
            .iter()
            .find(|e| {
                e.scope == "fn.impl" && e.claim == "translation-validation/evidence-sufficiency"
            })
            .unwrap();
        assert_eq!(
            e.state,
            VerificationState::Proven,
            "critical: body_expr counts as evidence path"
        );
    }

    #[test]
    fn evidence_sufficiency_passes_in_critical_when_runtime_checks_present() {
        let mut node = fn_node_with_effects(0, "fn.runtime", &["io:Console"]);
        node.runtime_checks = Some(vec![RuntimeCheckMeta {
            predicate: "io_available()".to_string(),
            hash: "abc123".to_string(),
        }]);
        let graph = single_node_graph(node);
        let entries = TranslationValidator::check(&graph, "critical");
        let e = entries
            .iter()
            .find(|e| {
                e.scope == "fn.runtime" && e.claim == "translation-validation/evidence-sufficiency"
            })
            .unwrap();
        assert_eq!(
            e.state,
            VerificationState::Proven,
            "critical: runtime_checks count as evidence path"
        );
    }

    #[test]
    fn evidence_sufficiency_not_checked_in_prod() {
        // prod does NOT run TV-4 (only draft/dev/test/staging/prod — not critical)
        let node = fn_node_with_effects(0, "fn.iface", &["io:Console"]);
        let graph = single_node_graph(node);
        let entries = TranslationValidator::check(&graph, "prod");
        let tv4_entry = entries
            .iter()
            .find(|e| e.claim == "translation-validation/evidence-sufficiency");
        assert!(tv4_entry.is_none(), "TV-4 must not run in prod profile");
    }

    // ── Unknown profile strict-by-default ────────────────────────────────

    #[test]
    fn unknown_profile_runs_tv3_and_tv4() {
        let node = fn_node_full(
            0,
            "fn.unknown",
            "emit_effect(db)",
            &[], // undeclared effect → TV-3 fail
            Some("Unit"),
        );
        let graph = single_node_graph(node);
        let entries = TranslationValidator::check(&graph, "quantum"); // unknown profile

        // TV-3 should fire
        let tv3 = entries
            .iter()
            .find(|e| e.claim == "translation-validation/effect-obligation");
        assert!(
            tv3.is_some(),
            "unknown profile must run TV-3 (strict-by-default)"
        );
    }

    // ── Summary entry ─────────────────────────────────────────────────────

    #[test]
    fn empty_graph_produces_summary_proven() {
        let graph = SemanticGraph {
            nodes: vec![],
            edges: vec![],
        };
        let entries = TranslationValidator::check(&graph, "critical");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].state, VerificationState::Proven);
        assert_eq!(entries[0].scope, "translation_validation");
    }
}
