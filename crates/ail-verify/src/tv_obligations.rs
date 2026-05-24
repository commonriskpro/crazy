// ── ail-verify::tv_obligations ────────────────────────────────────────────
//
// TV-3 and TV-4 obligation helpers extracted from translation_validator.
//
// # Contents
//
// - `extract_body_effects`    — scan body_expr for emit/run/bind_effect calls
// - `effect_is_declared`      — prefix/exact match against declared effect_row
// - `check_effect_obligations`— TV-3: body effects must be declared (prod+)
// - `check_evidence_sufficiency`— TV-4: declared effects need evidence (critical+)
// - `is_prod_or_stricter`     — profile gate for TV-3
// - `is_critical_like`        — profile gate for TV-4
// - `make_entry`              — shared `VerificationEntry` constructor
//
// All functions are `pub(crate)`; nothing here is part of the public API.

use ail_core::semantic_graph::{NodeKind, SemanticGraph};

use crate::report::{VerificationEntry, VerificationState};

use crate::translation_validator::{E_TV_EFFECT_UNDECLARED, E_TV_INSUFFICIENT_EVIDENCE};

// ── TV-3: Effect obligations ──────────────────────────────────────────────

/// Extract effect identifiers referenced in a body expression.
///
/// Recognises `emit_effect(X)`, `run_effect(X)`, and `bind_effect(X)` as
/// effect references — these are the three canonical patterns used by the ANF
/// lowering stage and Stage 20 ordering checks.
///
/// Returns a deduplicated list of identifier strings found in the body.
///
/// # Scan assumptions
///
/// - Each keyword scan advances past the closing `)` after a match, skipping
///   re-scanning of already-consumed characters.  If no `)` is found after a
///   keyword, the scan resumes after the keyword itself.
/// - When no keyword is matched at the current position, the scanner advances
///   one byte at a time.
/// - Effect identifiers must consist solely of `[A-Za-z0-9_.:]+` characters;
///   any other character inside the parentheses causes the call to be ignored.
pub(crate) fn extract_body_effects(body: &str) -> Vec<String> {
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
                    // Advance past the closing paren to avoid re-scanning
                    // characters already consumed by this match.
                    pos = inner_start + close + 1;
                } else {
                    // No closing paren — skip past the keyword itself.
                    pos = inner_start;
                }
            } else {
                pos += 1;
            }
        }
    }

    found.into_iter().collect()
}

/// Return true if `declared_effects` covers the body effect identifier `id`.
///
/// A declared effect `"name:Provider"` covers identifier `"name"` (prefix
/// match before `:`) or the full string `"name:Provider"` (exact match).
pub(crate) fn effect_is_declared(id: &str, declared_effects: &[String]) -> bool {
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
pub(crate) fn check_effect_obligations(graph: &SemanticGraph) -> Vec<VerificationEntry> {
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
                vec![],
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
                vec![
                    "add the missing effect(s) to the function's effect_row declaration".into(),
                    "remove the effect call from the body if the effect is unintended".into(),
                ],
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
pub(crate) fn check_evidence_sufficiency(graph: &SemanticGraph) -> Vec<VerificationEntry> {
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
                vec![
                    "provide a body_expr with at least one effect call to serve as \
                     implementation evidence"
                        .into(),
                    "add runtime_checks entries to serve as evidence for the declared effects"
                        .into(),
                    "move the function to a less restrictive profile if critical evidence is not \
                     required"
                        .into(),
                ],
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
                vec![],
            ));
        }
    }

    entries
}

// ── Profile helpers ───────────────────────────────────────────────────────

/// Return true if `profile` is prod, staging, critical, or unrecognized
/// (strict-by-default).
pub(crate) fn is_prod_or_stricter(profile: &str) -> bool {
    matches!(profile, "prod" | "staging" | "critical")
        || !matches!(profile, "draft" | "dev" | "test")
}

/// Return true if `profile` is critical or unrecognized (strict-by-default).
pub(crate) fn is_critical_like(profile: &str) -> bool {
    profile == "critical" || !matches!(profile, "draft" | "dev" | "test" | "staging" | "prod")
}

// ── Entry constructor ─────────────────────────────────────────────────────

pub(crate) fn make_entry(
    claim: impl Into<String>,
    state: VerificationState,
    scope: impl Into<String>,
    evidence: Option<String>,
    repair_options: Vec<String>,
) -> VerificationEntry {
    let blocking = matches!(state, VerificationState::Failed | VerificationState::Unsafe);
    VerificationEntry {
        claim: claim.into(),
        state,
        scope: scope.into(),
        evidence,
        blocking,
        repair_options,
    }
}
