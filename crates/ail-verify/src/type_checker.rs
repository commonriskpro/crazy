// ── ail-verify::type_checker ──────────────────────────────────────────────
//
// Full type-system enforcement pass for the verification pipeline (step 7).
//
// # Scope (G24 round 2 — deeper enforcement)
//
// `TypeChecker::check` walks a `&SemanticGraph` and runs ten ordered subpasses:
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
// 8. **Boundary/inference materialization**:
//    - For each Function node with declared `params` and no `return_type`:
//      emit Unverified (E_BOUNDARY_NOT_MATERIALIZED, claim "boundary-materialization").
//    - Both present → Proven.
//
// 9. **Null/absence policy**:
//    - If any node's `return_type` equals "null", "nil", "undefined", or "void"
//      (case-insensitive) → Failed (E_NULL_IN_CORE_IR, claim "null-policy").
//
// 10. **Float equality/ordering policy**:
//    - If a Type node's `type_facts.nominal == "Float"` AND has_eq=true
//      → Failed (E_FLOAT_EQ_IMPLICIT, claim "float-policy").
//    - If nominal == "Float" AND has_ord=true
//      → Failed (E_FLOAT_ORD_IMPLICIT, claim "float-policy").
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
/// Function has declared params but no return_type — boundary not yet materialized.
pub const E_BOUNDARY_NOT_MATERIALIZED: &str = "E_BOUNDARY_NOT_MATERIALIZED";
/// Return type contains null/nil/undefined/void — prohibited in Core IR.
pub const E_NULL_IN_CORE_IR: &str = "E_NULL_IN_CORE_IR";
/// Float type has implicit equality (has_eq=true) without explicit comparator policy.
pub const E_FLOAT_EQ_IMPLICIT: &str = "E_FLOAT_EQ_IMPLICIT";
/// Float type has implicit ordering (has_ord=true) — Float has no default Ord.
pub const E_FLOAT_ORD_IMPLICIT: &str = "E_FLOAT_ORD_IMPLICIT";
/// Associated type binding has empty concrete type (ty field is empty).
pub const E_ASSOC_TYPE_EMPTY_BINDING: &str = "E_ASSOC_TYPE_EMPTY_BINDING";
/// Generic call-site bindings do not match the callee declaration arity.
pub const E_GENERIC_BINDING_ARITY: &str = "E_GENERIC_BINDING_ARITY";
/// Callee effects are not propagated to the caller effect row.
pub const E_EFFECT_NOT_PROPAGATED: &str = "E_EFFECT_NOT_PROPAGATED";
/// Callee capabilities are not propagated to the caller capability requirements.
pub const E_CAPABILITY_NOT_PROPAGATED: &str = "E_CAPABILITY_NOT_PROPAGATED";
/// Structural type requirement is not satisfied by the concrete argument type.
pub const E_STRUCTURAL_TYPE_MISMATCH: &str = "E_STRUCTURAL_TYPE_MISMATCH";
/// Dynamic interface dispatch target does not implement the requested interface.
pub const E_DYN_INTERFACE_UNAVAILABLE: &str = "E_DYN_INTERFACE_UNAVAILABLE";
/// Refinement status claims proof but predicate cannot be discharged locally.
pub const E_REFINEMENT_PROOF_UNDISCHARGED: &str = "E_REFINEMENT_PROOF_UNDISCHARGED";
/// Runtime-checked refinement has no materialized runtime check metadata.
pub const E_REFINEMENT_RUNTIME_CHECK_MISSING: &str = "E_REFINEMENT_RUNTIME_CHECK_MISSING";
/// PatchField type carries an empty inner type — prohibited in Core IR.
pub const E_PATCHFIELD_EMPTY_INNER: &str = "E_PATCHFIELD_EMPTY_INNER";
/// Boundary inferred fact return type does not match declared return_type.
pub const E_BOUNDARY_INFERENCE_MISMATCH: &str = "E_BOUNDARY_INFERENCE_MISMATCH";
/// Type used in a context requiring PartialOrd but only partial order is available.
pub const E_PARTIAL_ORD_REQUIRED: &str = "E_PARTIAL_ORD_REQUIRED";

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

        // Subpass 3b — call-site generic arity/binding validation.
        Self::check_generic_call_bindings(graph, &ctx, &mut entries);

        // Subpass 3c — effects and capabilities must propagate across calls.
        Self::check_effect_capability_propagation(graph, &ctx, &mut entries);

        // Subpass 4 — variance enforcement.
        Self::check_variance(graph, &ctx, &mut entries);

        // Subpass 4b — structural/Dyn interface call-site checks.
        Self::check_structural_and_dyn_calls(graph, &ctx, &mut entries);

        // Subpass 5 — interface coherence.
        Self::check_interface_coherence(graph, &mut entries);

        // Subpass 6 — constraint enforcement.
        Self::check_constraints(graph, &ctx, &mut entries);

        // Subpass 7 — refinement proof obligations.
        Self::check_refinements(graph, &mut entries);

        // Subpass 8 — boundary/inference materialization.
        Self::check_boundary_materialization(graph, &mut entries);

        // Subpass 9 — null/absence policy.
        Self::check_null_policy(graph, &mut entries);

        // Subpass 10 — float equality/ordering policy.
        Self::check_float_policy(graph, &mut entries);

        // Subpass 11 — PatchField inner-type validation.
        Self::check_patchfield(graph, &mut entries);

        // Subpass 12 — PartialOrd vs Ord distinction.
        Self::check_partial_ord(graph, &mut entries);

        // Subpass 13 — Boundary inference cross-check.
        Self::check_boundary_inference(graph, &mut entries);

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
                        evidence: Some("E_GENERIC_ARITY: generic parameter name is empty".into()),
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

    fn check_generic_call_bindings(
        graph: &SemanticGraph,
        ctx: &TypeContext<'_>,
        entries: &mut Vec<VerificationEntry>,
    ) {
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

            let declared: Vec<&str> = generic_params
                .iter()
                .filter(|p| p.kind == GenericParamKind::TypeParam)
                .map(|p| p.name.as_str())
                .collect();
            let scope = format!("{}→{}", edge.source.0, callee.name);

            let unknown = bindings
                .iter()
                .find(|b| !declared.iter().any(|name| *name == b.param));
            if let Some(binding) = unknown {
                entries.push(VerificationEntry {
                    claim: "generic-call-binding".into(),
                    state: VerificationState::Failed,
                    scope,
                    evidence: Some(format!(
                        "{E_GENERIC_BINDING_ARITY}: call binds unknown generic '{}' on '{}'",
                        binding.param, callee.name
                    )),
                });
                continue;
            }

            if bindings.len() != declared.len() {
                entries.push(VerificationEntry {
                    claim: "generic-call-binding".into(),
                    state: VerificationState::Failed,
                    scope,
                    evidence: Some(format!(
                        "{E_GENERIC_BINDING_ARITY}: '{}' expects {} type generic bindings, got {}",
                        callee.name,
                        declared.len(),
                        bindings.len()
                    )),
                });
            } else {
                entries.push(VerificationEntry {
                    claim: "generic-call-binding".into(),
                    state: VerificationState::Proven,
                    scope,
                    evidence: None,
                });
            }
        }
    }

    fn check_effect_capability_propagation(
        graph: &SemanticGraph,
        ctx: &TypeContext<'_>,
        entries: &mut Vec<VerificationEntry>,
    ) {
        for edge in &graph.edges {
            if edge.kind != EdgeKind::Calls {
                continue;
            }
            let Some(caller) = ctx.by_ref.get(&edge.source).copied() else {
                continue;
            };
            let Some(callee) = ctx.by_ref.get(&edge.target).copied() else {
                continue;
            };
            let scope = format!("{}→{}", caller.name, callee.name);

            if let Some(callee_effects) = &callee.effect_row {
                let caller_effects = caller
                    .effect_row
                    .as_ref()
                    .map(|row| row.effects.as_slice())
                    .unwrap_or(&[]);
                if let Some(missing) = callee_effects
                    .effects
                    .iter()
                    .find(|effect| !caller_effects.iter().any(|e| e == *effect))
                {
                    entries.push(VerificationEntry {
                        claim: "effect-propagation".into(),
                        state: VerificationState::Failed,
                        scope: scope.clone(),
                        evidence: Some(format!(
                            "{E_EFFECT_NOT_PROPAGATED}: callee effect '{}' from '{}' is missing from caller '{}'",
                            missing, callee.name, caller.name
                        )),
                    });
                } else if !callee_effects.effects.is_empty() {
                    entries.push(VerificationEntry {
                        claim: "effect-propagation".into(),
                        state: VerificationState::Proven,
                        scope: scope.clone(),
                        evidence: None,
                    });
                }
            }

            if let Some(callee_caps) = &callee.capability_reqs {
                let caller_caps = caller
                    .capability_reqs
                    .as_ref()
                    .map(|reqs| reqs.caps.as_slice())
                    .unwrap_or(&[]);
                if let Some(missing) = callee_caps
                    .caps
                    .iter()
                    .find(|cap| !caller_caps.iter().any(|c| c == *cap))
                {
                    entries.push(VerificationEntry {
                        claim: "capability-propagation".into(),
                        state: VerificationState::Failed,
                        scope: scope.clone(),
                        evidence: Some(format!(
                            "{E_CAPABILITY_NOT_PROPAGATED}: callee capability '{}' from '{}' is missing from caller '{}'",
                            missing, callee.name, caller.name
                        )),
                    });
                } else if !callee_caps.caps.is_empty() {
                    entries.push(VerificationEntry {
                        claim: "capability-propagation".into(),
                        state: VerificationState::Proven,
                        scope: scope.clone(),
                        evidence: None,
                    });
                }
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

    fn check_structural_and_dyn_calls(
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

            for (idx, (arg_ty, param)) in call_args.iter().zip(params.iter()).enumerate() {
                let scope = format!("{}→{}[{}]", edge.source.0, callee.name, idx);
                if let Some(required) = structural_fields(&param.ty) {
                    if structural_type_satisfies(ctx, arg_ty, &required) {
                        entries.push(VerificationEntry {
                            claim: "structural-type".into(),
                            state: VerificationState::Proven,
                            scope: scope.clone(),
                            evidence: None,
                        });
                    } else {
                        entries.push(VerificationEntry {
                            claim: "structural-type".into(),
                            state: VerificationState::Failed,
                            scope: scope.clone(),
                            evidence: Some(format!(
                                "{E_STRUCTURAL_TYPE_MISMATCH}: argument type '{}' does not satisfy structural requirement '{}'",
                                arg_ty, param.ty
                            )),
                        });
                    }
                }

                if let Some(interface) = dyn_interface(&param.ty) {
                    let implements = ctx
                        .get_by_name(arg_ty)
                        .and_then(|node| node.interface_impls.as_ref())
                        .map(|impls| impls.iter().any(|impl_| impl_.interface == interface))
                        .unwrap_or(false);
                    entries.push(VerificationEntry {
                        claim: "dyn-interface".into(),
                        state: if implements {
                            VerificationState::Proven
                        } else {
                            VerificationState::Failed
                        },
                        scope,
                        evidence: if implements {
                            None
                        } else {
                            Some(format!(
                                "{E_DYN_INTERFACE_UNAVAILABLE}: argument type '{}' has no impl for Dyn<{}>",
                                arg_ty, interface
                            ))
                        },
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
                // Check associated type bindings for empty names or empty concrete types.
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
                    if at.ty.is_empty() {
                        entries.push(VerificationEntry {
                            claim: "coherence".into(),
                            state: VerificationState::Failed,
                            scope: node.name.clone(),
                            evidence: Some(format!(
                                "{E_ASSOC_TYPE_EMPTY_BINDING}: associated type binding '{}' \
                                 has no concrete type in impl of '{}' on '{}'; \
                                 associated types must be explicit in the IR",
                                at.name, impl_.interface, node.name
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
                let scope = format!(
                    "{}→{}[{}={}]",
                    edge.source.0, callee.name, binding.param, binding.ty
                );
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
        Self::emit_constraint_check_for_scope(
            ctx, type_arg, &scope, needs_eq, needs_hash, needs_ord, entries,
        );
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

    // ── Subpass 8: Boundary/inference materialization ─────────────────────

    /// Check that Function nodes with declared params also declare a return type.
    ///
    /// The canonical Semantic Graph must store fully resolved signatures for
    /// all public boundaries.  A function with declared params but no return
    /// type has an incomplete boundary — it has not been materialized yet.
    ///
    /// - `params` present and non-empty + `return_type` absent → Unverified (E_BOUNDARY_NOT_MATERIALIZED)
    /// - `params` present and `return_type` present → Proven ("boundary-materialization")
    /// - No `params` → skipped (no boundary-materialization entry)
    fn check_boundary_materialization(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
        for node in &graph.nodes {
            if node.kind != NodeKind::Function {
                continue;
            }
            let Some(params) = &node.params else {
                continue; // no params declared — skip
            };
            if params.is_empty() {
                continue; // empty params list — skip
            }
            let scope = node.name.clone();
            if node.return_type.is_none() {
                entries.push(VerificationEntry {
                    claim: "boundary-materialization".into(),
                    state: VerificationState::Unverified,
                    scope,
                    evidence: Some(format!(
                        "{E_BOUNDARY_NOT_MATERIALIZED}: function '{}' declares params \
                         but has no return_type; boundary signature is not fully materialized \
                         in the canonical graph",
                        node.name
                    )),
                });
            } else {
                entries.push(VerificationEntry {
                    claim: "boundary-materialization".into(),
                    state: VerificationState::Proven,
                    scope,
                    evidence: None,
                });
            }
        }
    }

    // ── Subpass 9: Null/absence policy ────────────────────────────────────

    /// Enforce the null/absence policy from docs/type-system.md:
    /// "No null/nil/undefined in Core IR."
    ///
    /// If any node's `return_type` is literally "null", "nil", "undefined",
    /// or "void" (case-insensitive), fail with E_NULL_IN_CORE_IR.
    ///
    /// Domain absence must be modeled with `Option<T>`, failures with
    /// `Result<T, E>`, and partial updates with `PatchField<T>`.
    fn check_null_policy(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
        const NULL_WORDS: &[&str] = &["null", "nil", "undefined", "void"];
        for node in &graph.nodes {
            let Some(return_type) = &node.return_type else {
                continue;
            };
            let lower = return_type.to_lowercase();
            if NULL_WORDS.iter().any(|&w| lower == w) {
                entries.push(VerificationEntry {
                    claim: "null-policy".into(),
                    state: VerificationState::Failed,
                    scope: node.name.clone(),
                    evidence: Some(format!(
                        "{E_NULL_IN_CORE_IR}: return_type '{}' of '{}' is a null/nil sentinel; \
                         Core IR prohibits null — use Option<T>, Result<T,E>, or PatchField<T>",
                        return_type, node.name
                    )),
                });
            }
        }
    }

    // ── Subpass 10: Float equality/ordering policy ─────────────────────────

    /// Enforce Float-specific equality and ordering rules from docs/type-system.md:
    ///
    /// - "Float equality requires explicit approximate/bitwise/domain comparator."
    /// - "Float has no default Ord."
    ///
    /// A Type node whose `type_facts.nominal == "Float"` (the raw Float primitive,
    /// not a refinement like NonNaNFloat) must NOT declare `has_eq = true` or
    /// `has_ord = true` in its constraint_set.
    ///
    /// Refinements of Float (e.g., `NonNaNFloat` with nominal "NonNaNFloat") are
    /// exempt — they may define explicit comparison semantics.
    fn check_float_policy(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
        for node in &graph.nodes {
            let Some(tf) = &node.type_facts else {
                continue;
            };
            // Only apply to nodes whose nominal IS exactly "Float".
            if tf.nominal != "Float" {
                continue;
            }
            let Some(cs) = &node.constraint_set else {
                continue; // no constraints declared — nothing to check
            };
            let scope = node.name.clone();
            if cs.has_eq {
                entries.push(VerificationEntry {
                    claim: "float-policy".into(),
                    state: VerificationState::Failed,
                    scope: scope.clone(),
                    evidence: Some(format!(
                        "{E_FLOAT_EQ_IMPLICIT}: Float type '{}' declares has_eq=true; \
                         Float equality must be explicit (approximately_equal, bitwise_equal, \
                         or a domain-specific comparator) — not implicit `==`",
                        node.name
                    )),
                });
            }
            if cs.has_ord {
                entries.push(VerificationEntry {
                    claim: "float-policy".into(),
                    state: VerificationState::Failed,
                    scope,
                    evidence: Some(format!(
                        "{E_FLOAT_ORD_IMPLICIT}: Float type '{}' declares has_ord=true; \
                         Float has no default total order (NaN breaks totality) — \
                         use NonNaNFloat or an explicit comparator/wrapper instead",
                        node.name
                    )),
                });
            }
        }
    }

    // ── Subpass 11: PatchField inner-type validation ─────────────────────

    /// Enforce that `PatchField<T>` always carries a non-empty inner type.
    ///
    /// `PatchField<>` is prohibited in Core IR — partial updates must
    /// target a concrete type.
    ///
    /// - `return_type` starts with "PatchField<" and inner is non-empty → Proven
    /// - `return_type` starts with "PatchField<" and inner is empty → Failed (E_PATCHFIELD_EMPTY_INNER)
    /// - All other return types are not touched by this subpass.
    fn check_patchfield(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
        for node in &graph.nodes {
            let Some(return_type) = &node.return_type else {
                continue;
            };
            if !return_type.starts_with("PatchField<") {
                continue;
            }
            let (_, inner) = split_generic(return_type);
            let scope = node.name.clone();
            if inner.is_empty() {
                entries.push(VerificationEntry {
                    claim: "patchfield".into(),
                    state: VerificationState::Failed,
                    scope,
                    evidence: Some(format!(
                        "{E_PATCHFIELD_EMPTY_INNER}: PatchField on '{}' has no inner type; \
                         Core IR requires PatchField<T> where T is a non-empty concrete type",
                        node.name
                    )),
                });
            } else {
                entries.push(VerificationEntry {
                    claim: "patchfield".into(),
                    state: VerificationState::Proven,
                    scope,
                    evidence: None,
                });
            }
        }
    }

    // ── Subpass 12: PartialOrd vs Ord distinction ─────────────────────────

    /// Distinguish nodes that carry `has_partial_ord = true` in their
    /// constraint set.
    ///
    /// - `return_type` starts with "PartialOrd<" AND `has_partial_ord = true`
    ///   → Proven ("partial-ord") — the node is used in an explicit partial-order
    ///   context and satisfies it.
    /// - `has_partial_ord = true` AND `has_ord = false` in any other ordering
    ///   context → Unverified ("partial-ord") with E_PARTIAL_ORD_REQUIRED —
    ///   informational: total-order contexts cannot be served by partial order alone.
    fn check_partial_ord(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
        for node in &graph.nodes {
            let Some(cs) = &node.constraint_set else {
                continue;
            };
            if !cs.has_partial_ord {
                continue;
            }
            let Some(return_type) = &node.return_type else {
                continue;
            };
            let scope = node.name.clone();
            if return_type.starts_with("PartialOrd<") {
                entries.push(VerificationEntry {
                    claim: "partial-ord".into(),
                    state: VerificationState::Proven,
                    scope,
                    evidence: None,
                });
            } else if !cs.has_ord {
                entries.push(VerificationEntry {
                    claim: "partial-ord".into(),
                    state: VerificationState::Unverified,
                    scope,
                    evidence: Some(format!(
                        "{E_PARTIAL_ORD_REQUIRED}: type '{}' has has_partial_ord=true \
                         but lacks has_ord=true; in a total-order context only partial \
                         ordering is available, which may be insufficient",
                        node.name
                    )),
                });
            }
        }
    }

    // ── Subpass 13: Boundary inference cross-check ────────────────────────

    /// Cross-check inferred boundary facts against the declared return type.
    ///
    /// For each Function node with `inferred` facts of `kind == "boundary"`:
    /// - Parse the `value` as `"return:TYPE"`.
    /// - Compare the extracted TYPE to `node.return_type`.
    /// - Match → Proven ("boundary-inference").
    /// - Mismatch → Failed ("boundary-inference") with E_BOUNDARY_INFERENCE_MISMATCH.
    /// - No boundary facts → no entry emitted (subpass is skipped for that node).
    fn check_boundary_inference(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
        for node in &graph.nodes {
            if node.kind != NodeKind::Function {
                continue;
            }
            for fact in &node.inferred {
                if fact.kind != "boundary" {
                    continue;
                }
                let Some(claimed_return) = fact.value.strip_prefix("return:") else {
                    continue;
                };
                let scope = node.name.clone();
                let declared = node.return_type.as_deref().unwrap_or("");
                if claimed_return == declared {
                    entries.push(VerificationEntry {
                        claim: "boundary-inference".into(),
                        state: VerificationState::Proven,
                        scope,
                        evidence: None,
                    });
                } else {
                    entries.push(VerificationEntry {
                        claim: "boundary-inference".into(),
                        state: VerificationState::Failed,
                        scope,
                        evidence: Some(format!(
                            "{E_BOUNDARY_INFERENCE_MISMATCH}: boundary inferred return \
                             type '{claimed_return}' does not match declared return_type \
                             '{declared}' on '{}'",
                            node.name
                        )),
                    });
                }
            }
        }
    }

    // ── Subpass 7: Refinement proof obligations ───────────────────────────

    fn check_refinements(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
        for node in &graph.nodes {
            let Some(rf) = &node.refinement_ref else {
                continue;
            };

            // Map RefinementStatus to VerificationState.
            let mut state = match rf.status {
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
            let mut evidence = format!("predicate: '{}'; base: '{}'", rf.predicate, rf.base_type);

            if matches!(
                rf.status,
                ail_core::semantic_graph::RefinementStatus::Proven
            ) {
                match rf.predicate.trim() {
                    "true" => {}
                    "false" | "" => {
                        state = VerificationState::Failed;
                        evidence = format!(
                            "{E_REFINEMENT_PROOF_UNDISCHARGED}: proven refinement '{}' cannot be discharged locally",
                            rf.predicate
                        );
                    }
                    _ => {
                        if node.contract_clauses.as_ref().map(|clauses| {
                            clauses
                                .ensures
                                .iter()
                                .any(|p| p.trim() == rf.predicate.trim())
                        }) != Some(true)
                        {
                            state = VerificationState::Unverified;
                            evidence = format!(
                                "{E_REFINEMENT_PROOF_UNDISCHARGED}: refinement '{}' has no matching ensures clause or literal proof",
                                rf.predicate
                            );
                        }
                    }
                }
            }

            if matches!(
                rf.status,
                ail_core::semantic_graph::RefinementStatus::RuntimeChecked
            ) && node
                .runtime_checks
                .as_ref()
                .map(|checks| checks.is_empty())
                .unwrap_or(true)
            {
                state = VerificationState::Failed;
                evidence = format!(
                    "{E_REFINEMENT_RUNTIME_CHECK_MISSING}: runtime-checked refinement '{}' has no materialized runtime check",
                    rf.predicate
                );
            }

            entries.push(VerificationEntry {
                claim: "refinement".into(),
                state,
                scope: node.name.clone(),
                evidence: Some(evidence),
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
    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')
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

fn dyn_interface(ty: &str) -> Option<&str> {
    ty.strip_prefix("Dyn<")
        .and_then(|rest| rest.strip_suffix('>'))
        .map(str::trim)
        .filter(|interface| !interface.is_empty())
}

fn structural_fields(ty: &str) -> Option<Vec<String>> {
    let body = ty
        .strip_prefix("struct{")
        .and_then(|rest| rest.strip_suffix('}'))?;
    let fields: Vec<String> = body
        .split(',')
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .map(str::to_string)
        .collect();
    if fields.is_empty() {
        None
    } else {
        Some(fields)
    }
}

fn structural_type_satisfies(ctx: &TypeContext<'_>, arg_ty: &str, required: &[String]) -> bool {
    if let Some(actual) = structural_fields(arg_ty) {
        return required
            .iter()
            .all(|field| actual.iter().any(|a| a == field));
    }
    let Some(node) = ctx.get_by_name(arg_ty) else {
        return false;
    };
    let Some(constraints) = &node.constraint_set else {
        return false;
    };
    required.iter().all(|field| {
        constraints.extras.iter().any(|extra| {
            extra == field
                || extra
                    .strip_prefix("field:")
                    .map(|declared| declared == field)
                    .unwrap_or(false)
        })
    })
}

/// Build `SummaryCounts` from the entry list.
fn build_summary_counts(entries: &[VerificationEntry]) -> SummaryCounts {
    SummaryCounts {
        verified_count: entries
            .iter()
            .filter(|e| {
                e.state == VerificationState::Proven || e.state == VerificationState::RuntimeChecked
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
