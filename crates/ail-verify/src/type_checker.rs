// ── ail-verify::type_checker ──────────────────────────────────────────────
//
// Full type-system enforcement pass for the verification pipeline (step 7).
//
// # Scope (G24 — full enforcement)
//
// `TypeChecker::check` walks a `&SemanticGraph` and runs seven ordered subpasses:
//
// 1. **Nominal presence** (backward-compat with pre-G24 graphs):
//    - Function/Type nodes with non-empty `TypeFacts.nominal` + valid generics → Proven
//    - Empty generic name in `TypeFacts.generics` → Failed (E_GENERIC_ARITY)
//    - `TypeFacts` absent or empty nominal → Unverified
//    - All other node kinds are skipped.
//
// 2. **Nominal call check**:
//    - For each `Calls` edge with `call_args`: verify arg types match the
//      callee's `params` type declarations in order.
//    - Mismatch → Failed (E_NOMINAL_MISMATCH, claim "nominal-call").
//    - Match → Proven (claim "nominal-call").
//
// 3. **Generic param kind validation**:
//    - For each node with `generic_params`:
//      - Empty name → Failed (E_GENERIC_ARITY, claim "generic-param-kind").
//      - `EffectParam` name not in `effect_row.effects` → Failed (E_EFFECT_PARAM_WIDENED).
//      - `CapabilityParam` name not in `capability_reqs.caps` → Failed (E_CAPABILITY_PARAM_WIDENED).
//      - Valid params → Proven (claim "generic-param-kind").
//
// 4. **Variance enforcement**:
//    - For each `Calls` edge with `call_args`: if both the call arg and the
//      callee param are parameterized types (contain `<`), and the base types
//      match but the type arguments differ → Failed (E_VARIANCE_COERCION, claim "variance").
//
// 5. **Interface coherence**:
//    - For each node with `interface_impls`:
//      - Duplicate impl of the same interface (non-adapter) → Failed (E_COHERENCE_DUPLICATE).
//      - Associated type binding with empty name → Failed (E_ASSOC_TYPE_MISMATCH).
//    - Claim "coherence".
//
// 6. **Constraint enforcement**:
//    - Collection types with constrained semantics (`Set`, `Map`, `OrderedSet`,
//      `OrderedMap`) require their type-param nodes to have the declared constraints.
//      - `Set<T>` / `Map<K,_>` require `Eq + Hashable` → E_MISSING_HASH / E_MISSING_EQ.
//      - `OrderedSet<T>` / `OrderedMap<K,_>` require `Ord` → E_MISSING_ORD.
//    - Generic functions with `required_constraints` on type params, validated
//      at call sites via `type_arg_bindings`.
//    - Claim "constraint-check".
//
// 7. **Refinement proof obligations**:
//    - For each node with `refinement_ref`: emit a "refinement" entry with
//      the state that matches `refinement_ref.status`.
//    - If `refinement_ref.erased == true`: also emit a "refinement-erasure" entry
//      (Assumed state, documenting the explicit erasure).
//
// # Determinism
//
// Subpasses run in fixed order; entries are emitted in graph-traversal order
// within each subpass.  Two calls with identical input produce identical output.
//
// # Exclusions
//
// - No SMT/Z3 calls.
// - No runtime execution.
// - No I/O or mutation.
// - Policy acceptance decisions remain in `PolicyEngine`.

use std::collections::BTreeMap;

use ail_core::semantic_graph::{
    EdgeKind, GenericParamKind, GraphNode, NodeKind, NodeRef, SemanticGraph,
};

use crate::report::{SummaryCounts, VerificationEntry, VerificationReport, VerificationState};

// ── Evidence codes ─────────────────────────────────────────────────────────

/// Nominal type mismatch at a call site.
pub const E_NOMINAL_MISMATCH: &str = "E_NOMINAL_MISMATCH";
/// Generic parameter name is empty (arity error).
pub const E_GENERIC_ARITY: &str = "E_GENERIC_ARITY";
/// EffectParam is declared but not reflected in the node's `effect_row`.
pub const E_EFFECT_PARAM_WIDENED: &str = "E_EFFECT_PARAM_WIDENED";
/// CapabilityParam is declared but not reflected in the node's `capability_reqs`.
pub const E_CAPABILITY_PARAM_WIDENED: &str = "E_CAPABILITY_PARAM_WIDENED";
/// Implicit coercion between parameterized types (variance violation).
pub const E_VARIANCE_COERCION: &str = "E_VARIANCE_COERCION";
/// Duplicate non-adapter implementation of the same interface on one node.
pub const E_COHERENCE_DUPLICATE: &str = "E_COHERENCE_DUPLICATE";
/// Associated type binding has an empty or invalid name.
pub const E_ASSOC_TYPE_MISMATCH: &str = "E_ASSOC_TYPE_MISMATCH";
/// Type used in a context requiring `Eq` but `has_eq` is false.
pub const E_MISSING_EQ: &str = "E_MISSING_EQ";
/// Type used in a context requiring `Hashable` but `has_hash` is false.
pub const E_MISSING_HASH: &str = "E_MISSING_HASH";
/// Type used in a context requiring `Ord` but `has_ord` is false.
pub const E_MISSING_ORD: &str = "E_MISSING_ORD";
/// Refinement erasure to base type occurred and is explicitly reported.
pub const E_REFINEMENT_ERASURE: &str = "E_REFINEMENT_ERASURE";
/// ConstParam name contains a complex/undecidable expression.
pub const E_CONST_PARAM_UNDECIDABLE: &str = "E_CONST_PARAM_UNDECIDABLE";

// ── Collection constraint table ───────────────────────────────────────────

/// Which constraints are required on the first type parameter of each
/// standard collection type.
///
/// `(nominal, requires_eq, requires_hash, requires_ord)`
const COLLECTION_CONSTRAINTS: &[(&str, bool, bool, bool)] = &[
    ("Set", true, true, false),
    ("Map", true, true, false),
    ("OrderedSet", true, false, true),
    ("OrderedMap", true, false, true),
];

// ── TypeContext ────────────────────────────────────────────────────────────

/// Collected type facts indexed for efficient lookup.
///
/// Populated in a single pass over the graph.  All lookups are deterministic
/// (`BTreeMap` keys).
struct TypeContext<'a> {
    /// Nodes indexed by `NodeRef`.
    by_ref: BTreeMap<NodeRef, &'a GraphNode>,
    /// Nodes indexed by name.
    by_name: BTreeMap<&'a str, NodeRef>,
}

impl<'a> TypeContext<'a> {
    fn collect(graph: &'a SemanticGraph) -> Self {
        let mut by_ref = BTreeMap::new();
        let mut by_name = BTreeMap::new();
        for node in &graph.nodes {
            by_ref.insert(node.id, node);
            by_name.insert(node.name.as_str(), node.id);
        }
        TypeContext { by_ref, by_name }
    }

    fn get_by_name(&self, name: &str) -> Option<&GraphNode> {
        self.by_name
            .get(name)
            .and_then(|id| self.by_ref.get(id))
            .copied()
    }
}

// ── TypeChecker ───────────────────────────────────────────────────────────

/// Pure, stateless type checker.
///
/// Runs seven ordered subpasses over a `&SemanticGraph` and aggregates
/// all `VerificationEntry` items into a single `VerificationReport`.
pub struct TypeChecker;

impl TypeChecker {
    /// Walk `graph` and run all seven type-checking subpasses.
    ///
    /// # Determinism
    ///
    /// Two calls with identical input produce identical output.
    pub fn check(graph: &SemanticGraph) -> VerificationReport {
        let ctx = TypeContext::collect(graph);
        let mut entries: Vec<VerificationEntry> = Vec::new();

        // Subpass 1 — nominal presence (backward-compatible).
        Self::check_nominal_presence(graph, &mut entries);

        // Subpass 2 — nominal call check.
        Self::check_nominal_calls(graph, &ctx, &mut entries);

        // Subpass 3 — generic param kind validation.
        Self::check_generic_params(graph, &mut entries);

        // Subpass 4 — variance enforcement.
        Self::check_variance(graph, &ctx, &mut entries);

        // Subpass 5 — interface coherence.
        Self::check_interface_coherence(graph, &mut entries);

        // Subpass 6 — constraint enforcement.
        Self::check_constraints(graph, &ctx, &mut entries);

        // Subpass 7 — refinement proof obligations.
        Self::check_refinements(graph, &mut entries);

        let summary_counts = build_summary_counts(&entries);
        VerificationReport {
            entries,
            schema_version: "verification/1.0".into(),
            summary_counts,
            ..Default::default()
        }
    }

    // ── Subpass 1: Nominal presence ───────────────────────────────────────

    fn check_nominal_presence(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
        for node in &graph.nodes {
            if !matches!(node.kind, NodeKind::Function | NodeKind::Type) {
                continue;
            }
            entries.push(Self::classify_nominal_presence(node));
        }
    }

    fn classify_nominal_presence(node: &GraphNode) -> VerificationEntry {
        let scope = node.name.clone();

        match &node.type_facts {
            None => VerificationEntry {
                claim: "type-check".into(),
                state: VerificationState::Unverified,
                scope,
                evidence: None,
            },
            Some(tf) if tf.nominal.is_empty() => VerificationEntry {
                claim: "type-check".into(),
                state: VerificationState::Unverified,
                scope,
                evidence: None,
            },
            Some(tf) => {
                let bad_generic = tf.generics.iter().any(|g| g.is_empty());
                if bad_generic {
                    VerificationEntry {
                        claim: "type-check".into(),
                        state: VerificationState::Failed,
                        scope,
                        evidence: Some(
                            "E_GENERIC_ARITY: generic parameter name is empty".into(),
                        ),
                    }
                } else {
                    VerificationEntry {
                        claim: "type-check".into(),
                        state: VerificationState::Proven,
                        scope,
                        evidence: None,
                    }
                }
            }
        }
    }

    // ── Subpass 2: Nominal call check ─────────────────────────────────────

    fn check_nominal_calls(
        graph: &SemanticGraph,
        ctx: &TypeContext<'_>,
        entries: &mut Vec<VerificationEntry>,
    ) {
        for edge in &graph.edges {
            if edge.kind != EdgeKind::Calls {
                continue;
            }
            let Some(call_args) = &edge.call_args else {
                continue;
            };
            // Look up callee.
            let Some(callee) = ctx.by_ref.get(&edge.target).copied() else {
                continue;
            };
            let Some(params) = &callee.params else {
                continue;
            };
            // Compare arg types to param types pairwise.
            for (i, (arg_ty, param)) in call_args.iter().zip(params.iter()).enumerate() {
                let scope = format!("{}→{}[{}]", edge.source.0, callee.name, i);
                if arg_ty == &param.ty {
                    entries.push(VerificationEntry {
                        claim: "nominal-call".into(),
                        state: VerificationState::Proven,
                        scope,
                        evidence: None,
                    });
                } else {
                    entries.push(VerificationEntry {
                        claim: "nominal-call".into(),
                        state: VerificationState::Failed,
                        scope,
                        evidence: Some(format!(
                            "{E_NOMINAL_MISMATCH}: expected '{}', got '{arg_ty}' \
                             at param '{}' of '{}'",
                            param.ty, param.name, callee.name
                        )),
                    });
                }
            }
        }
    }

    // ── Subpass 3: Generic param kind validation ──────────────────────────

    fn check_generic_params(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
        for node in &graph.nodes {
            let Some(generic_params) = &node.generic_params else {
                continue;
            };
            for gp in generic_params {
                let scope = format!("{}::{}", node.name, gp.name);

                // Empty name → arity error.
                if gp.name.is_empty() {
                    entries.push(VerificationEntry {
                        claim: "generic-param-kind".into(),
                        state: VerificationState::Failed,
                        scope,
                        evidence: Some(format!(
                            "{E_GENERIC_ARITY}: generic parameter name is empty \
                             on node '{}'",
                            node.name
                        )),
                    });
                    continue;
                }

                // Kind-specific checks.
                let state_and_evidence: (VerificationState, Option<String>) = match gp.kind {
                    GenericParamKind::EffectParam => {
                        // EffectParam name must appear in the node's effect_row.
                        let in_row = node
                            .effect_row
                            .as_ref()
                            .map(|er| er.effects.iter().any(|e| e == &gp.name))
                            .unwrap_or(false);
                        if in_row {
                            (VerificationState::Proven, None)
                        } else {
                            (
                                VerificationState::Failed,
                                Some(format!(
                                    "{E_EFFECT_PARAM_WIDENED}: EffectParam '{}' \
                                     not found in effect_row of '{}'; \
                                     effect precision would be silently widened",
                                    gp.name, node.name
                                )),
                            )
                        }
                    }
                    GenericParamKind::CapabilityParam => {
                        // CapabilityParam name must appear in capability_reqs.
                        let in_caps = node
                            .capability_reqs
                            .as_ref()
                            .map(|cr| cr.caps.iter().any(|c| c == &gp.name))
                            .unwrap_or(false);
                        if in_caps {
                            (VerificationState::Proven, None)
                        } else {
                            (
                                VerificationState::Failed,
                                Some(format!(
                                    "{E_CAPABILITY_PARAM_WIDENED}: CapabilityParam '{}' \
                                     not found in capability_reqs of '{}'; \
                                     capability precision would be silently widened",
                                    gp.name, node.name
                                )),
                            )
                        }
                    }
                    GenericParamKind::TypeParam => {
                        // TypeParam: no additional validation at the declaration
                        // site (constraint requirements are checked at call
                        // sites in subpass 6).
                        (VerificationState::Proven, None)
                    }
                    GenericParamKind::ConstParam => {
                        // ConstParam must stay decidable: the name must be a
                        // simple identifier (letters, digits, underscore — no
                        // spaces, operators, or function-call syntax).
                        if is_simple_identifier(&gp.name) {
                            (VerificationState::Proven, None)
                        } else {
                            (
                                VerificationState::Failed,
                                Some(format!(
                                    "{E_CONST_PARAM_UNDECIDABLE}: ConstParam '{}' \
                                     contains a complex expression; only simple \
                                     decidable identifiers are permitted",
                                    gp.name
                                )),
                            )
                        }
                    }
                };

                entries.push(VerificationEntry {
                    claim: "generic-param-kind".into(),
                    state: state_and_evidence.0,
                    scope,
                    evidence: state_and_evidence.1,
                });
            }
        }
    }

    // ── Subpass 4: Variance enforcement ──────────────────────────────────

    fn check_variance(
        graph: &SemanticGraph,
        ctx: &TypeContext<'_>,
        entries: &mut Vec<VerificationEntry>,
    ) {
        for edge in &graph.edges {
            if edge.kind != EdgeKind::Calls {
                continue;
            }
            let Some(call_args) = &edge.call_args else {
                continue;
            };
            let Some(callee) = ctx.by_ref.get(&edge.target).copied() else {
                continue;
            };
            let Some(params) = &callee.params else {
                continue;
            };

            for (i, (arg_ty, param)) in call_args.iter().zip(params.iter()).enumerate() {
                // Only check parameterized types (those containing '<').
                if !arg_ty.contains('<') || !param.ty.contains('<') {
                    continue;
                }

                let (arg_base, arg_inner) = split_generic(arg_ty);
                let (param_base, param_inner) = split_generic(&param.ty);

                // Bases must match for this to be a variance attempt
                // (different bases are caught by subpass 2 as a nominal mismatch).
                if arg_base != param_base {
                    continue;
                }

                // Same base but different type argument → variance coercion attempt.
                if arg_inner != param_inner {
                    let scope = format!("{}→{}[{}]", edge.source.0, callee.name, i);
                    entries.push(VerificationEntry {
                        claim: "variance".into(),
                        state: VerificationState::Failed,
                        scope,
                        evidence: Some(format!(
                            "{E_VARIANCE_COERCION}: implicit coercion from \
                             '{arg_ty}' to '{}' violates invariance; \
                             use an explicit adapter/constraint instead",
                            param.ty
                        )),
                    });
                }
            }
        }
    }

    // ── Subpass 5: Interface coherence ────────────────────────────────────

    fn check_interface_coherence(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
        for node in &graph.nodes {
            let Some(impls) = &node.interface_impls else {
                continue;
            };

            // Check for duplicate non-adapter implementations of the same interface.
            let mut seen_non_adapter: BTreeMap<&str, usize> = BTreeMap::new();
            for (idx, impl_) in impls.iter().enumerate() {
                // Check associated type bindings for empty names.
                for at in &impl_.associated_types {
                    if at.name.is_empty() {
                        entries.push(VerificationEntry {
                            claim: "coherence".into(),
                            state: VerificationState::Failed,
                            scope: node.name.clone(),
                            evidence: Some(format!(
                                "{E_ASSOC_TYPE_MISMATCH}: associated type binding \
                                 has empty name in impl of '{}' on '{}'",
                                impl_.interface, node.name
                            )),
                        });
                    }
                }

                if impl_.is_adapter {
                    continue; // adapter exception
                }

                if let Some(first_idx) = seen_non_adapter.get(impl_.interface.as_str()).copied() {
                    // Duplicate non-adapter impl detected.
                    entries.push(VerificationEntry {
                        claim: "coherence".into(),
                        state: VerificationState::Failed,
                        scope: node.name.clone(),
                        evidence: Some(format!(
                            "{E_COHERENCE_DUPLICATE}: duplicate non-adapter \
                             implementation #{idx} of '{}' on '{}' \
                             (first at #{first_idx}); \
                             ambiguous impl must fail deterministically",
                            impl_.interface, node.name
                        )),
                    });
                } else {
                    seen_non_adapter.insert(&impl_.interface, idx);
                }
            }
        }
    }

    // ── Subpass 6: Constraint enforcement ────────────────────────────────

    fn check_constraints(
        graph: &SemanticGraph,
        ctx: &TypeContext<'_>,
        entries: &mut Vec<VerificationEntry>,
    ) {
        // 6a: Collection type constraint requirements.
        for node in &graph.nodes {
            let Some(tf) = &node.type_facts else {
                continue;
            };
            if tf.generics.is_empty() {
                continue;
            }
            for &(coll_nominal, needs_eq, needs_hash, needs_ord) in COLLECTION_CONSTRAINTS {
                if tf.nominal != coll_nominal {
                    continue;
                }
                // Check the first generic arg (the element/key type).
                let type_arg = &tf.generics[0];
                Self::emit_constraint_check(
                    ctx,
                    type_arg,
                    node.name.as_str(),
                    needs_eq,
                    needs_hash,
                    needs_ord,
                    entries,
                );
                break; // only one collection rule applies per node
            }
        }

        // 6b: Call-site generic instantiation constraint checks.
        for edge in &graph.edges {
            if edge.kind != EdgeKind::Calls {
                continue;
            }
            let Some(bindings) = &edge.type_arg_bindings else {
                continue;
            };
            let Some(callee) = ctx.by_ref.get(&edge.target).copied() else {
                continue;
            };
            let Some(generic_params) = &callee.generic_params else {
                continue;
            };

            for binding in bindings {
                // Find the generic param declaration for this binding.
                let Some(gp) = generic_params.iter().find(|p| p.name == binding.param) else {
                    continue;
                };
                if gp.required_constraints.is_empty() {
                    continue;
                }
                // Look up the concrete type node.
                let needs_eq = gp.required_constraints.iter().any(|c| c == "Eq");
                let needs_hash = gp.required_constraints.iter().any(|c| c == "Hashable");
                let needs_ord = gp.required_constraints.iter().any(|c| c == "Ord");
                let scope = format!("{}→{}[{}={}]", edge.source.0, callee.name, binding.param, binding.ty);
                Self::emit_constraint_check_for_scope(
                    ctx,
                    &binding.ty,
                    &scope,
                    needs_eq,
                    needs_hash,
                    needs_ord,
                    entries,
                );
            }
        }
    }

    fn emit_constraint_check(
        ctx: &TypeContext<'_>,
        type_arg: &str,
        node_name: &str,
        needs_eq: bool,
        needs_hash: bool,
        needs_ord: bool,
        entries: &mut Vec<VerificationEntry>,
    ) {
        let scope = format!("{node_name}<{type_arg}>");
        Self::emit_constraint_check_for_scope(ctx, type_arg, &scope, needs_eq, needs_hash, needs_ord, entries);
    }

    fn emit_constraint_check_for_scope(
        ctx: &TypeContext<'_>,
        type_name: &str,
        scope: &str,
        needs_eq: bool,
        needs_hash: bool,
        needs_ord: bool,
        entries: &mut Vec<VerificationEntry>,
    ) {
        // If the type arg node is not in the graph (e.g., it is a generic
        // type parameter name like "K" or "T" rather than a concrete declared
        // type), we cannot verify its constraints — skip silently.
        let Some(type_node) = ctx.get_by_name(type_name) else {
            return;
        };
        let cs = type_node.constraint_set.as_ref();

        let has_eq = cs.map(|c| c.has_eq).unwrap_or(false);
        let has_hash = cs.map(|c| c.has_hash).unwrap_or(false);
        let has_ord = cs.map(|c| c.has_ord).unwrap_or(false);

        let mut evidence_parts: Vec<String> = Vec::new();

        if needs_eq && !has_eq {
            evidence_parts.push(format!(
                "{E_MISSING_EQ}: type '{type_name}' requires Eq constraint"
            ));
        }
        if needs_hash && !has_hash {
            evidence_parts.push(format!(
                "{E_MISSING_HASH}: type '{type_name}' requires Hashable constraint"
            ));
        }
        if needs_ord && !has_ord {
            evidence_parts.push(format!(
                "{E_MISSING_ORD}: type '{type_name}' requires Ord constraint"
            ));
        }

        if evidence_parts.is_empty() {
            entries.push(VerificationEntry {
                claim: "constraint-check".into(),
                state: VerificationState::Proven,
                scope: scope.to_string(),
                evidence: None,
            });
        } else {
            entries.push(VerificationEntry {
                claim: "constraint-check".into(),
                state: VerificationState::Failed,
                scope: scope.to_string(),
                evidence: Some(evidence_parts.join("; ")),
            });
        }
    }

    // ── Subpass 7: Refinement proof obligations ───────────────────────────

    fn check_refinements(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
        for node in &graph.nodes {
            let Some(rf) = &node.refinement_ref else {
                continue;
            };

            // Map RefinementStatus to VerificationState.
            let state = match rf.status {
                ail_core::semantic_graph::RefinementStatus::Proven => VerificationState::Proven,
                ail_core::semantic_graph::RefinementStatus::RuntimeChecked => {
                    VerificationState::RuntimeChecked
                }
                ail_core::semantic_graph::RefinementStatus::Assumed => VerificationState::Assumed,
                ail_core::semantic_graph::RefinementStatus::Unverified => {
                    VerificationState::Unverified
                }
                ail_core::semantic_graph::RefinementStatus::Failed => VerificationState::Failed,
            };

            entries.push(VerificationEntry {
                claim: "refinement".into(),
                state,
                scope: node.name.clone(),
                evidence: Some(format!(
                    "predicate: '{}'; base: '{}'",
                    rf.predicate, rf.base_type
                )),
            });

            // Emit explicit erasure entry if the refinement was downgraded.
            if rf.erased {
                entries.push(VerificationEntry {
                    claim: "refinement-erasure".into(),
                    state: VerificationState::Assumed,
                    scope: node.name.clone(),
                    evidence: Some(format!(
                        "{E_REFINEMENT_ERASURE}: refinement '{}' \
                         erased to base type '{}'; \
                         erasure is explicit and tracked",
                        rf.predicate, rf.base_type
                    )),
                });
            }
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Returns `true` when `name` is a simple decidable identifier:
/// letters, digits, or underscores only — no spaces, operators, or brackets.
fn is_simple_identifier(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_')
}

/// Split `"Base<Inner>"` into `("Base", "Inner")`.
///
/// Returns `(input, "")` if the input is not a parameterized type.
fn split_generic(ty: &str) -> (&str, &str) {
    if let Some(lt) = ty.find('<') {
        let base = ty[..lt].trim_end();
        let inner = ty[lt + 1..].trim_end_matches('>').trim();
        (base, inner)
    } else {
        (ty, "")
    }
}

/// Build `SummaryCounts` from the entry list.
fn build_summary_counts(entries: &[VerificationEntry]) -> SummaryCounts {
    SummaryCounts {
        verified_count: entries
            .iter()
            .filter(|e| {
                e.state == VerificationState::Proven
                    || e.state == VerificationState::RuntimeChecked
            })
            .count(),
        runtime_checked_count: entries
            .iter()
            .filter(|e| e.state == VerificationState::RuntimeChecked)
            .count(),
        assumed_count: entries
            .iter()
            .filter(|e| e.state == VerificationState::Assumed)
            .count(),
        unverified_count: entries
            .iter()
            .filter(|e| e.state == VerificationState::Unverified)
            .count(),
        unsafe_count: entries
            .iter()
            .filter(|e| e.state == VerificationState::Unsafe)
            .count(),
        failed_count: entries
            .iter()
            .filter(|e| e.state == VerificationState::Failed)
            .count(),
    }
}
