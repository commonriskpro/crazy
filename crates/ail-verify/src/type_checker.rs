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
    EffectArgBinding, CapabilityArgBinding,
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
/// Associated type reference in a callee return type could not be resolved
/// to a concrete type via any impl binding in the call context.
pub const E_ASSOC_TYPE_NOT_RESOLVED: &str = "E_ASSOC_TYPE_NOT_RESOLVED";
/// Function node has a ForeignType return type but no `boundary-schema` inferred
/// fact — the foreign value has no declared serialization schema.
pub const E_FOREIGN_TYPE_NO_SCHEMA: &str = "E_FOREIGN_TYPE_NO_SCHEMA";
/// Two non-adapter Type nodes both implement the same interface with overlapping
/// type families, creating a blanket impl coherence conflict.
pub const E_BLANKET_IMPL_OVERLAP: &str = "E_BLANKET_IMPL_OVERLAP";
/// A non-adapter implementation declares a foreign interface but neither the
/// interface name nor the implementing type name appears in the graph as an
/// owned node — orphan rule violation.
pub const E_ORPHAN_RULE_VIOLATION: &str = "E_ORPHAN_RULE_VIOLATION";
/// An EffectParam instantiation binding specifies an effect not present in
/// the caller's effect_row — the caller cannot supply that effect.
pub const E_EFFECT_PARAM_NOT_THREADED: &str = "E_EFFECT_PARAM_NOT_THREADED";
/// A CapabilityParam instantiation binding specifies a capability not present
/// in the caller's capability_reqs — the caller cannot supply that capability.
pub const E_CAPABILITY_PARAM_NOT_THREADED: &str = "E_CAPABILITY_PARAM_NOT_THREADED";
/// ConstParam instantiation value is not a decidable literal (not a simple
/// numeric string or simple identifier).
pub const E_CONST_PARAM_VALUE_INVALID: &str = "E_CONST_PARAM_VALUE_INVALID";

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
        // Pre-pass: infer local return types for Function nodes without an
        // explicit `return_type`.  Results feed into boundary-materialization
        // so that inferred signatures suppress E_BOUNDARY_NOT_MATERIALIZED.
        let inferred_returns = Self::infer_local_types(graph);

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
        Self::check_boundary_materialization(graph, &inferred_returns, &mut entries);

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

        // Subpass 14 — Associated type resolution at call sites.
        Self::check_associated_type_resolution(graph, &ctx, &mut entries);

        // Subpass 15 — ConstParam value validation at call sites (extends Subpass 3b).
        Self::check_const_param_call_bindings(graph, &ctx, &mut entries);

        // Subpass 16 — ForeignType boundary schema enforcement.
        Self::check_boundary_schema(graph, &mut entries);

        // Subpass 18 — Effect and capability parameter threading at call sites.
        Self::check_effect_capability_param_threading(graph, &ctx, &mut entries);

        // Subpass 17 — Blanket impl coherence and orphan rule.
        Self::check_blanket_impl_coherence(graph, &ctx, &mut entries);

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
                blocking: false,
                repair_options: vec![],
            },
            Some(tf) if tf.nominal.is_empty() => VerificationEntry {
                claim: "type-check".into(),
                state: VerificationState::Unverified,
                scope,
                evidence: None,
                blocking: false,
                repair_options: vec![],
            },
            Some(tf) => {
                let bad_generic = tf.generics.iter().any(|g| g.is_empty());
                if bad_generic {
                    VerificationEntry {
                        claim: "type-check".into(),
                        state: VerificationState::Failed,
                        scope,
                        evidence: Some("E_GENERIC_ARITY: generic parameter name is empty".into()),
                        blocking: true,
                        repair_options: vec![],
                    }
                } else {
                    VerificationEntry {
                        claim: "type-check".into(),
                        state: VerificationState::Proven,
                        scope,
                        evidence: None,
                        blocking: true,
                        repair_options: vec![],
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
                        blocking: false,
                        repair_options: vec![],
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
                        blocking: true,
                        repair_options: vec![],
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
                        blocking: true,
                        repair_options: vec![],
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
                    blocking: false,
                    repair_options: vec![],
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

            let declared_type_params: Vec<&str> = generic_params
                .iter()
                .filter(|p| p.kind == GenericParamKind::TypeParam)
                .map(|p| p.name.as_str())
                .collect();

            let scope = format!("{}→{}", edge.source.0, callee.name);

            // Only check TypeParam bindings — ConstParam bindings are handled
            // in check_const_param_call_bindings (Subpass 15).
            let type_bindings: Vec<_> = bindings
                .iter()
                .filter(|b| {
                    // Keep only bindings that match a TypeParam declaration,
                    // or that do NOT match any ConstParam (i.e., truly unknown).
                    let is_const = generic_params
                        .iter()
                        .any(|p| p.name == b.param && p.kind == GenericParamKind::ConstParam);
                    !is_const
                })
                .collect();

            let unknown = type_bindings
                .iter()
                .find(|b| !declared_type_params.iter().any(|name| *name == b.param));
            if let Some(binding) = unknown {
                entries.push(VerificationEntry {
                    claim: "generic-call-binding".into(),
                    state: VerificationState::Failed,
                    scope,
                    evidence: Some(format!(
                        "{E_GENERIC_BINDING_ARITY}: call binds unknown generic '{}' on '{}'",
                        binding.param, callee.name
                    )),
                    blocking: true,
                    repair_options: vec![],
                });
                continue;
            }

            if type_bindings.len() != declared_type_params.len() {
                entries.push(VerificationEntry {
                    claim: "generic-call-binding".into(),
                    state: VerificationState::Failed,
                    scope,
                    evidence: Some(format!(
                        "{E_GENERIC_BINDING_ARITY}: '{}' expects {} type generic bindings, got {}",
                        callee.name,
                        declared_type_params.len(),
                        type_bindings.len()
                    )),
                    blocking: true,
                    repair_options: vec![],
                });
            } else if !type_bindings.is_empty() {
                // Only emit Proven when there are actual TypeParam bindings to check.
                entries.push(VerificationEntry {
                    claim: "generic-call-binding".into(),
                    state: VerificationState::Proven,
                    scope,
                    evidence: None,
                    blocking: false,
                    repair_options: vec![],
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
                        blocking: true,
                        repair_options: vec![],
                    });
                } else if !callee_effects.effects.is_empty() {
                    entries.push(VerificationEntry {
                        claim: "effect-propagation".into(),
                        state: VerificationState::Proven,
                        scope: scope.clone(),
                        evidence: None,
                        blocking: false,
                        repair_options: vec![],
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
                        blocking: true,
                        repair_options: vec![],
                    });
                } else if !callee_caps.caps.is_empty() {
                    entries.push(VerificationEntry {
                        claim: "capability-propagation".into(),
                        state: VerificationState::Proven,
                        scope: scope.clone(),
                        evidence: None,
                        blocking: false,
                        repair_options: vec![],
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
                        blocking: true,
                        repair_options: vec![],
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
                            blocking: false,
                            repair_options: vec![],
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
                            blocking: true,
                            repair_options: vec![],
                        });
                    }
                }

                if let Some(interface) = dyn_interface(&param.ty) {
                    let implements = ctx
                        .get_by_name(arg_ty)
                        .and_then(|node| node.interface_impls.as_ref())
                        .map(|impls| impls.iter().any(|impl_| impl_.interface == interface))
                        .unwrap_or(false);
                    let dyn_state = if implements {
                        VerificationState::Proven
                    } else {
                        VerificationState::Failed
                    };
                    entries.push(VerificationEntry {
                        claim: "dyn-interface".into(),
                        state: dyn_state,
                        scope,
                        evidence: if implements {
                            None
                        } else {
                            Some(format!(
                                "{E_DYN_INTERFACE_UNAVAILABLE}: argument type '{}' has no impl for Dyn<{}>",
                                arg_ty, interface
                            ))
                        },
                        blocking: !implements,
                        repair_options: vec![],
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
                            blocking: true,
                            repair_options: vec![],
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
                            blocking: true,
                            repair_options: vec![],
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
                        blocking: true,
                        repair_options: vec![],
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
                let needs_eq = gp.required_constraints.iter().any(|c| c.interface == "Eq");
                let needs_hash = gp.required_constraints.iter().any(|c| c.interface == "Hashable");
                let needs_ord = gp.required_constraints.iter().any(|c| c.interface == "Ord");
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
                blocking: false,
                repair_options: vec![],
            });
        } else {
            entries.push(VerificationEntry {
                claim: "constraint-check".into(),
                state: VerificationState::Failed,
                scope: scope.to_string(),
                evidence: Some(evidence_parts.join("; ")),
                blocking: true,
                repair_options: vec![],
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
    /// **Inference integration**: when `inferred_returns` contains an entry for
    /// a node (produced by [`infer_local_types`](Self::infer_local_types)), it
    /// is treated as if `return_type` were declared.  This suppresses
    /// `E_BOUNDARY_NOT_MATERIALIZED` for simple, locally-inferrable functions.
    ///
    /// - `params` present and non-empty + `return_type` absent + no inferred type
    ///   → Unverified (E_BOUNDARY_NOT_MATERIALIZED)
    /// - `params` present and (`return_type` present OR inferred type available)
    ///   → Proven ("boundary-materialization")
    /// - No `params` → skipped (no boundary-materialization entry)
    fn check_boundary_materialization(
        graph: &SemanticGraph,
        inferred_returns: &BTreeMap<NodeRef, String>,
        entries: &mut Vec<VerificationEntry>,
    ) {
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

            // Accept either a declared return type or a locally-inferred one.
            let has_return =
                node.return_type.is_some() || inferred_returns.contains_key(&node.id);

            if !has_return {
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
                    blocking: false,
                    repair_options: vec![],
                });
            } else {
                entries.push(VerificationEntry {
                    claim: "boundary-materialization".into(),
                    state: VerificationState::Proven,
                    scope,
                    evidence: None,
                    blocking: false,
                    repair_options: vec![],
                });
            }
        }
    }

    // ── Local type inference pre-pass ─────────────────────────────────────

    /// Infer return types for `Function` nodes that lack an explicit
    /// `return_type` but carry a `body_expr`.
    ///
    /// This is **local inference only** — no global unification or constraint
    /// solving.  The following expression forms are recognized:
    ///
    /// - **Int literal** — any `i64`-parseable string → `"Int"`
    /// - **Bool literal** — `"true"` or `"false"` → `"Bool"`
    /// - **Call to known function** — a bare name (or `name(...)` prefix)
    ///   whose return type is already declared in `ctx` → that return type
    /// - **If expression** — `"if ... else { BODY }"` where the else branch
    ///   is recursively inferrable → the else branch's type
    ///
    /// Inference is best-effort: if the body cannot be classified, the node
    /// is omitted from the returned map.
    fn infer_local_types(graph: &SemanticGraph) -> BTreeMap<NodeRef, String> {
        let ctx = TypeContext::collect(graph);
        let mut map: BTreeMap<NodeRef, String> = BTreeMap::new();

        for node in &graph.nodes {
            // Only infer for Function nodes without a declared return type.
            if node.kind != NodeKind::Function || node.return_type.is_some() {
                continue;
            }
            let Some(body) = &node.body_expr else {
                continue;
            };
            if let Some(ty) = infer_expr_type(body.trim(), &ctx) {
                map.insert(node.id, ty);
            }
        }

        map
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
                    blocking: true,
                    repair_options: vec![],
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
                    blocking: true,
                    repair_options: vec![],
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
                    blocking: true,
                    repair_options: vec![],
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
                    blocking: true,
                    repair_options: vec![],
                });
            } else {
                entries.push(VerificationEntry {
                    claim: "patchfield".into(),
                    state: VerificationState::Proven,
                    scope,
                    evidence: None,
                    blocking: false,
                    repair_options: vec![],
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
                    blocking: false,
                    repair_options: vec![],
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
                    blocking: false,
                    repair_options: vec![],
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
                        blocking: false,
                        repair_options: vec![],
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
                        blocking: true,
                        repair_options: vec![],
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
                blocking: false,
                repair_options: vec![],
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
                    blocking: false,
                    repair_options: vec![],
                });
            }
        }
    }

    // ── Subpass 14: Associated type resolution ────────────────────────────

    /// For each `Calls` edge, if the callee's `return_type` contains "::"
    /// (indicating a reference to an interface associated type), scan the
    /// caller's `call_args` for a `Type` node whose `interface_impls` resolves
    /// the association.
    ///
    /// - Resolved → Proven (claim "assoc-type-resolution"), evidence contains concrete type
    /// - Unresolvable → Unverified (E_ASSOC_TYPE_NOT_RESOLVED)
    fn check_associated_type_resolution(
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
            let Some(return_type) = &callee.return_type else {
                continue;
            };
            // Only process associated type references (contain "::").
            if !return_type.contains("::") {
                continue;
            }

            let scope = format!("{}→{}", edge.source.0, callee.name);

            // Split "Interface::AssocName" into the interface base and assoc name.
            let (interface_base, assoc_name) = split_assoc_type(return_type);

            // Scan call_args for a Type node whose interface_impls can resolve
            // the associated type.
            let resolved = call_args.iter().find_map(|arg_ty| {
                resolve_assoc_type(ctx, arg_ty, interface_base, assoc_name)
            });

            match resolved {
                Some(concrete_ty) => {
                    entries.push(VerificationEntry {
                        claim: "assoc-type-resolution".into(),
                        state: VerificationState::Proven,
                        scope,
                        evidence: Some(format!(
                            "resolved '{return_type}' → '{concrete_ty}'"
                        )),
                        blocking: false,
                        repair_options: vec![],
                    });
                }
                None => {
                    entries.push(VerificationEntry {
                        claim: "assoc-type-resolution".into(),
                        state: VerificationState::Unverified,
                        scope,
                        evidence: Some(format!(
                            "{E_ASSOC_TYPE_NOT_RESOLVED}: \
                             associated type '{return_type}' on callee '{}' \
                             could not be resolved from call_args {call_args:?}",
                            callee.name
                        )),
                        blocking: false,
                        repair_options: vec![],
                    });
                }
            }
        }
    }

    // ── Subpass 18: Effect and capability parameter threading ─────────────

    /// For each `Calls` edge with `effect_arg_bindings` or `capability_arg_bindings`,
    /// verify that the caller can supply the required effects / capabilities.
    ///
    /// **Effect threading**: for each `EffectArgBinding`, every effect listed in
    /// `binding.effects` must appear in the caller's `effect_row.effects`.
    /// - All present → Proven (claim "effect-param-threading")
    /// - Any missing → Failed (E_EFFECT_PARAM_NOT_THREADED)
    ///
    /// **Capability threading**: for each `CapabilityArgBinding`, every cap listed
    /// in `binding.caps` must appear in the caller's `capability_reqs.caps`.
    /// - All present → Proven (claim "capability-param-threading")
    /// - Any missing → Failed (E_CAPABILITY_PARAM_NOT_THREADED)
    fn check_effect_capability_param_threading(
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

            // ── Effect param threading ──────────────────────────────────
            if let Some(effect_bindings) = &edge.effect_arg_bindings {
                let caller_effects = caller
                    .effect_row
                    .as_ref()
                    .map(|row| row.effects.as_slice())
                    .unwrap_or(&[]);

                let mut all_ok = true;
                for binding in effect_bindings {
                    for effect in &binding.effects {
                        if !caller_effects.iter().any(|e| e == effect) {
                            entries.push(VerificationEntry {
                                claim: "effect-param-threading".into(),
                                state: VerificationState::Failed,
                                scope: scope.clone(),
                                evidence: Some(format!(
                                    "{E_EFFECT_PARAM_NOT_THREADED}: EffectParam '{}' requires \
                                     effect '{}' which is not in caller '{}' effect_row {:?}",
                                    binding.param, effect, caller.name, caller_effects
                                )),
                                blocking: true,
                                repair_options: vec![],
                            });
                            all_ok = false;
                        }
                    }
                }
                if all_ok {
                    entries.push(VerificationEntry {
                        claim: "effect-param-threading".into(),
                        state: VerificationState::Proven,
                        scope: scope.clone(),
                        evidence: None,
                        blocking: false,
                        repair_options: vec![],
                    });
                }
            }

            // ── Capability param threading ──────────────────────────────
            if let Some(cap_bindings) = &edge.capability_arg_bindings {
                let caller_caps = caller
                    .capability_reqs
                    .as_ref()
                    .map(|reqs| reqs.caps.as_slice())
                    .unwrap_or(&[]);

                let mut all_ok = true;
                for binding in cap_bindings {
                    for cap in &binding.caps {
                        if !caller_caps.iter().any(|c| c == cap) {
                            entries.push(VerificationEntry {
                                claim: "capability-param-threading".into(),
                                state: VerificationState::Failed,
                                scope: scope.clone(),
                                evidence: Some(format!(
                                    "{E_CAPABILITY_PARAM_NOT_THREADED}: CapabilityParam '{}' \
                                     requires cap '{}' which is not in caller '{}' \
                                     capability_reqs {:?}",
                                    binding.param, cap, caller.name, caller_caps
                                )),
                                blocking: true,
                                repair_options: vec![],
                            });
                            all_ok = false;
                        }
                    }
                }
                if all_ok {
                    entries.push(VerificationEntry {
                        claim: "capability-param-threading".into(),
                        state: VerificationState::Proven,
                        scope: scope.clone(),
                        evidence: None,
                        blocking: false,
                        repair_options: vec![],
                    });
                }
            }
        }
    }

    // ── Subpass 16: ForeignType boundary schema enforcement ───────────────

    /// For each `Function` node whose `return_type` starts with `"ForeignType"`:
    /// - If the node has an `inferred` fact with `kind == "boundary-schema"` → Proven
    /// - Otherwise → Failed (E_FOREIGN_TYPE_NO_SCHEMA)
    fn check_boundary_schema(graph: &SemanticGraph, entries: &mut Vec<VerificationEntry>) {
        for node in &graph.nodes {
            if node.kind != NodeKind::Function {
                continue;
            }
            let Some(return_type) = &node.return_type else {
                continue;
            };
            if !return_type.starts_with("ForeignType") {
                continue;
            }

            let has_schema = node
                .inferred
                .iter()
                .any(|fact| fact.kind == "boundary-schema");

            let scope = node.name.clone();
            if has_schema {
                entries.push(VerificationEntry {
                    claim: "boundary-schema".into(),
                    state: VerificationState::Proven,
                    scope,
                    evidence: None,
                    blocking: false,
                    repair_options: vec![],
                });
            } else {
                entries.push(VerificationEntry {
                    claim: "boundary-schema".into(),
                    state: VerificationState::Failed,
                    scope,
                    evidence: Some(format!(
                        "{E_FOREIGN_TYPE_NO_SCHEMA}: function '{}' returns a ForeignType \
                         ('{}') but has no 'boundary-schema' inferred fact; \
                         foreign values crossing boundaries must declare a serialization schema",
                        node.name, return_type
                    )),
                    blocking: true,
                    repair_options: vec![],
                });
            }
        }
    }

    // ── Subpass 17: Blanket impl coherence and orphan rule ────────────────

    /// Check two coherence invariants across all Type nodes in the graph:
    ///
    /// 1. **Blanket impl overlap**: if two distinct non-adapter Type nodes both
    ///    declare `interface_impls` with the same interface name, that is an
    ///    overlap → Failed (E_BLANKET_IMPL_OVERLAP).
    ///
    /// 2. **Orphan rule**: a non-adapter impl's interface must be "owned" by the
    ///    graph — either an `Interface` node with that name or a `Type` node with
    ///    that name must exist.  If neither is present → Failed (E_ORPHAN_RULE_VIOLATION).
    fn check_blanket_impl_coherence(
        graph: &SemanticGraph,
        ctx: &TypeContext<'_>,
        entries: &mut Vec<VerificationEntry>,
    ) {
        // Pass 1 — collect all (interface_name, type_node_name) pairs for
        // non-adapter impls.  Use a BTreeMap<interface, Vec<node_name>> to
        // detect duplicates deterministically.
        let mut impl_map: BTreeMap<String, Vec<String>> = BTreeMap::new();

        for node in &graph.nodes {
            let Some(impls) = &node.interface_impls else {
                continue;
            };
            for impl_meta in impls {
                if impl_meta.is_adapter {
                    continue; // adapter exception
                }
                impl_map
                    .entry(impl_meta.interface.clone())
                    .or_default()
                    .push(node.name.clone());
            }
        }

        // Emit overlap entries.
        for (interface, node_names) in &impl_map {
            if node_names.len() > 1 {
                entries.push(VerificationEntry {
                    claim: "blanket-impl-coherence".into(),
                    state: VerificationState::Failed,
                    scope: node_names.join(","),
                    evidence: Some(format!(
                        "{E_BLANKET_IMPL_OVERLAP}: interface '{}' has overlapping \
                         non-adapter impls on nodes {:?}; \
                         blanket impl coherence violation",
                        interface, node_names
                    )),
                    blocking: true,
                    repair_options: vec![],
                });
            }
        }

        // Pass 2 — orphan rule check.
        for node in &graph.nodes {
            let Some(impls) = &node.interface_impls else {
                continue;
            };
            for impl_meta in impls {
                if impl_meta.is_adapter {
                    continue; // adapter exception
                }
                // The interface is "owned" if there is an Interface node
                // or a Type node with that name in the graph.
                let interface_owned = ctx.get_by_name(&impl_meta.interface).map_or(false, |n| {
                    matches!(n.kind, NodeKind::Interface | NodeKind::Type)
                });
                if !interface_owned {
                    entries.push(VerificationEntry {
                        claim: "orphan-rule".into(),
                        state: VerificationState::Failed,
                        scope: node.name.clone(),
                        evidence: Some(format!(
                            "{E_ORPHAN_RULE_VIOLATION}: node '{}' implements interface '{}' \
                             (is_adapter=false) but neither an Interface nor a Type node \
                             named '{}' exists in the graph; \
                             orphan rule requires the interface or type to be declared locally",
                            node.name, impl_meta.interface, impl_meta.interface
                        )),
                        blocking: true,
                        repair_options: vec![],
                    });
                }
            }
        }
    }

    // ── Subpass 15: ConstParam value validation at call sites ─────────────

    /// For each `Calls` edge with `type_arg_bindings`, check bindings that
    /// correspond to a `ConstParam` declaration on the callee.
    ///
    /// A valid ConstParam value must be a decidable literal: a numeric string
    /// (all digit characters) or a simple identifier (letters, digits, underscore).
    ///
    /// - Valid value → Proven (claim "const-param-value")
    /// - Invalid value → Failed (E_CONST_PARAM_VALUE_INVALID)
    fn check_const_param_call_bindings(
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

            for binding in bindings {
                // Only process ConstParam bindings.
                let is_const = generic_params
                    .iter()
                    .any(|p| p.name == binding.param && p.kind == GenericParamKind::ConstParam);
                if !is_const {
                    continue;
                }

                let scope = format!(
                    "{}→{}[{}={}]",
                    edge.source.0, callee.name, binding.param, binding.ty
                );

                if is_const_param_value(&binding.ty) {
                    entries.push(VerificationEntry {
                        claim: "const-param-value".into(),
                        state: VerificationState::Proven,
                        scope,
                        evidence: None,
                        blocking: false,
                        repair_options: vec![],
                    });
                } else {
                    entries.push(VerificationEntry {
                        claim: "const-param-value".into(),
                        state: VerificationState::Failed,
                        scope,
                        evidence: Some(format!(
                            "{E_CONST_PARAM_VALUE_INVALID}: ConstParam '{}' on '{}' \
                             bound to '{}' which is not a decidable literal; \
                             only numeric strings or simple identifiers are permitted",
                            binding.param, callee.name, binding.ty
                        )),
                        blocking: true,
                        repair_options: vec![],
                    });
                }
            }
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

// ── Local type inference ──────────────────────────────────────────────────

/// Attempt to infer a return type from a `body_expr` string using local
/// pattern matching.  Returns `None` when the expression form is unrecognized.
fn infer_expr_type(body: &str, ctx: &TypeContext<'_>) -> Option<String> {
    // Int literal: any valid i64 string (e.g. "42", "-1", "0").
    if body.parse::<i64>().is_ok() {
        return Some("Int".to_string());
    }

    // Bool literal.
    if body == "true" || body == "false" {
        return Some("Bool".to_string());
    }

    // If expression: look for the else branch and infer from it.
    if body.starts_with("if ") || body.starts_with("if(") {
        if let Some(inferred) = infer_if_expr_type(body, ctx) {
            return Some(inferred);
        }
    }

    // Call to a known function: bare name or "name(...)" prefix.
    // Extract the identifier before the first "(" or whitespace.
    let callee_name = body
        .split(|c: char| c == '(' || c.is_whitespace())
        .next()
        .unwrap_or(body)
        .trim();

    if !callee_name.is_empty() {
        if let Some(node) = ctx.get_by_name(callee_name) {
            if let Some(rt) = &node.return_type {
                return Some(rt.clone());
            }
        }
    }

    None
}

/// Infer the type of an if-expression by examining its else branch.
///
/// Handles the simple form `"if <cond> { <then> } else { <else> }"`.
/// Returns the inferred type of the else branch, or `None` if the else
/// branch cannot be located or its type cannot be inferred.
fn infer_if_expr_type(body: &str, ctx: &TypeContext<'_>) -> Option<String> {
    // Find the "else" keyword.
    let else_pos = body.find("else")?;
    let after_else = body[else_pos + 4..].trim();

    // Strip surrounding braces if present: "{ BODY }" → "BODY".
    let else_body = if after_else.starts_with('{') {
        after_else
            .strip_prefix('{')
            .and_then(|s| s.strip_suffix('}'))
            .map(str::trim)
            .unwrap_or(after_else)
    } else {
        after_else
    };

    infer_expr_type(else_body, ctx)
}

/// Returns `true` when `name` is a simple decidable identifier:
/// letters, digits, or underscores only — no spaces, operators, or brackets.
fn is_simple_identifier(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Returns `true` when `ty` is a valid decidable ConstParam value:
/// either an all-digit numeric literal (e.g., `"3"`, `"16"`) or a simple
/// identifier (e.g., `"MAX_SIZE"`).
fn is_const_param_value(ty: &str) -> bool {
    if ty.is_empty() {
        return false;
    }
    // Numeric literal: all digit characters.
    if ty.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    // Simple identifier: alphanumeric + underscore, no operators/parens/spaces.
    is_simple_identifier(ty)
}

/// Split an associated type reference `"Interface::AssocName"` into
/// `("Interface", "AssocName")`.
///
/// Returns `(input, "")` if the input does not contain `"::"`.
fn split_assoc_type(ty: &str) -> (&str, &str) {
    if let Some(pos) = ty.find("::") {
        (&ty[..pos], &ty[pos + 2..])
    } else {
        (ty, "")
    }
}

/// Attempt to resolve an associated type for `arg_ty` by scanning the
/// `interface_impls` of the corresponding node in `ctx`.
///
/// Looks for an `InterfaceImplMeta` whose `interface` starts with
/// `interface_base` (handles both exact and generic interface names),
/// then finds an `AssociatedTypeBinding` with `name == assoc_name`.
///
/// Returns `Some(concrete_ty)` on success, `None` if unresolvable.
fn resolve_assoc_type<'a>(
    ctx: &TypeContext<'a>,
    arg_ty: &str,
    interface_base: &str,
    assoc_name: &str,
) -> Option<String> {
    let node = ctx.get_by_name(arg_ty)?;
    let impls = node.interface_impls.as_ref()?;
    impls.iter().find_map(|impl_meta| {
        // Match if the impl's interface starts with the interface base
        // (handles both "Repository" and "Repository<User>" forms).
        if impl_meta.interface.starts_with(interface_base) {
            impl_meta
                .associated_types
                .iter()
                .find(|at| at.name == assoc_name)
                .map(|at| at.ty.clone())
        } else {
            None
        }
    })
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

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ail_core::semantic_graph::{
        AssociatedTypeBinding, CapabilityReqs, EffectRow, EdgeKind, GenericParamDecl,
        GenericParamKind, GraphEdge, GraphNode, InterfaceImplMeta, NodeKind, NodeRef,
        SemanticGraph, TypeArgBinding,
    };

    // ── helpers ───────────────────────────────────────────────────────────

    fn make_node(id: u32, kind: NodeKind, name: &str) -> GraphNode {
        GraphNode::new(NodeRef(id), kind, name)
    }

    fn make_edge(source: u32, target: u32) -> GraphEdge {
        GraphEdge::new(NodeRef(source), NodeRef(target), EdgeKind::Calls)
    }

    fn entries_with_claim<'a>(
        entries: &'a [VerificationEntry],
        claim: &str,
    ) -> Vec<&'a VerificationEntry> {
        entries.iter().filter(|e| e.claim == claim).collect()
    }

    // ── Task B1 (RED): check_associated_type_resolution ──────────────────
    // Tests written BEFORE the subpass exists — will fail to link at first.

    // B1-1: Proven case — associated type resolved via impl binding.
    // Spec scenario: "Associated type resolved via impl binding"
    //   GIVEN callee Function with return_type="Repository::Error"
    //   AND Type node "PostgresUserRepo" with interface_impls containing
    //     interface "Repository<User>", associated_types [{ name: "Error", ty: "DbError" }]
    //   AND Calls edge with call_args including "PostgresUserRepo"
    //   THEN entry claim "assoc-type-resolution", state Proven, evidence contains "DbError"
    #[test]
    fn assoc_type_resolved_via_impl_binding_is_proven() {
        let mut callee = make_node(1, NodeKind::Function, "load_user");
        callee.return_type = Some("Repository::Error".to_string());

        let mut impl_node = make_node(2, NodeKind::Type, "PostgresUserRepo");
        impl_node.interface_impls = Some(vec![InterfaceImplMeta {
            interface: "Repository<User>".to_string(),
            associated_types: vec![AssociatedTypeBinding {
                name: "Error".to_string(),
                ty: "DbError".to_string(),
            }],
            is_adapter: false,
        }]);

        let mut caller = make_node(0, NodeKind::Function, "caller_fn");
        caller.effect_row = None;

        let mut edge = make_edge(0, 1);
        edge.call_args = Some(vec!["PostgresUserRepo".to_string()]);

        let graph = SemanticGraph {
            nodes: vec![caller, callee, impl_node],
            edges: vec![edge],
        };

        let report = TypeChecker::check(&graph);
        let assoc = entries_with_claim(&report.entries, "assoc-type-resolution");
        assert!(
            !assoc.is_empty(),
            "at least one assoc-type-resolution entry expected"
        );
        let proven = assoc
            .iter()
            .find(|e| e.state == VerificationState::Proven);
        assert!(
            proven.is_some(),
            "expected a Proven assoc-type-resolution entry, got: {:?}",
            assoc
        );
        let ev = proven.unwrap().evidence.as_deref().unwrap_or("");
        assert!(
            ev.contains("DbError"),
            "evidence must contain the resolved type 'DbError', got: {ev}"
        );
    }

    // B1-2: Unverified case — associated type with no impl binding.
    // Spec scenario: "Associated type with no impl binding"
    //   GIVEN callee with return_type="Repository::Error"
    //   AND no matching impl in context
    //   THEN entry claim "assoc-type-resolution", state Unverified, evidence E_ASSOC_TYPE_NOT_RESOLVED
    #[test]
    fn assoc_type_unresolvable_is_unverified() {
        let mut callee = make_node(1, NodeKind::Function, "load_item");
        callee.return_type = Some("Repository::Error".to_string());

        let caller = make_node(0, NodeKind::Function, "no_impl_caller");

        let mut edge = make_edge(0, 1);
        // call_args references a type with NO interface_impls for Repository
        edge.call_args = Some(vec!["UnknownType".to_string()]);

        let graph = SemanticGraph {
            nodes: vec![caller, callee],
            edges: vec![edge],
        };

        let report = TypeChecker::check(&graph);
        let assoc = entries_with_claim(&report.entries, "assoc-type-resolution");
        assert!(
            !assoc.is_empty(),
            "expected assoc-type-resolution entry for unresolvable case"
        );
        let unverified = assoc
            .iter()
            .find(|e| e.state == VerificationState::Unverified);
        assert!(
            unverified.is_some(),
            "expected Unverified assoc-type-resolution entry, got: {:?}",
            assoc
        );
        let ev = unverified.unwrap().evidence.as_deref().unwrap_or("");
        assert!(
            ev.contains(E_ASSOC_TYPE_NOT_RESOLVED),
            "evidence must contain {E_ASSOC_TYPE_NOT_RESOLVED}, got: {ev}"
        );
    }

    // B1-3 (TRIANGULATE): Function with return_type that does NOT contain "::"
    // must NOT emit assoc-type-resolution entries.
    #[test]
    fn non_assoc_return_type_emits_no_assoc_type_resolution_entry() {
        let mut callee = make_node(1, NodeKind::Function, "get_count");
        callee.return_type = Some("Int".to_string()); // no "::"

        let caller = make_node(0, NodeKind::Function, "caller");
        let mut edge = make_edge(0, 1);
        edge.call_args = Some(vec!["SomeType".to_string()]);

        let graph = SemanticGraph {
            nodes: vec![caller, callee],
            edges: vec![edge],
        };

        let report = TypeChecker::check(&graph);
        let assoc = entries_with_claim(&report.entries, "assoc-type-resolution");
        assert!(
            assoc.is_empty(),
            "non-assoc return type must emit no assoc-type-resolution entries, got: {:?}",
            assoc
        );
    }

    // ── Task C3 (RED): check_effect_capability_param_threading ───────────
    // Tests written BEFORE the subpass exists.

    // C3-1: EffectParam effect present in caller → Proven.
    // Spec scenario: "EffectParam effect present in caller"
    //   GIVEN caller with effect_row={effects:["IO"]}
    //   AND Calls edge effect_arg_bindings=[{param:"e", effects:["IO"]}]
    //   THEN entry claim "effect-param-threading", state Proven
    #[test]
    fn effect_param_present_in_caller_is_proven() {
        use ail_core::semantic_graph::{EffectArgBinding, InferredFact};

        let mut caller = make_node(0, NodeKind::Function, "caller_fn");
        caller.effect_row = Some(EffectRow { effects: vec!["IO".to_string()] });

        let callee = make_node(1, NodeKind::Function, "effect_fn");

        let mut edge = make_edge(0, 1);
        edge.effect_arg_bindings = Some(vec![EffectArgBinding {
            param: "e".to_string(),
            effects: vec!["IO".to_string()],
        }]);

        let graph = SemanticGraph {
            nodes: vec![caller, callee],
            edges: vec![edge],
        };

        let report = TypeChecker::check(&graph);
        let threading = entries_with_claim(&report.entries, "effect-param-threading");
        assert!(
            !threading.is_empty(),
            "expected effect-param-threading entry"
        );
        let proven = threading.iter().find(|e| e.state == VerificationState::Proven);
        assert!(
            proven.is_some(),
            "effect present in caller must be Proven, got: {:?}",
            threading
        );
    }

    // C3-2: EffectParam effect MISSING from caller → Failed.
    // Spec scenario: "EffectParam effect missing from caller"
    //   GIVEN caller with effect_row={effects:[]}
    //   AND Calls edge effect_arg_bindings=[{param:"e", effects:["IO"]}]
    //   THEN entry claim "effect-param-threading", state Failed, evidence E_EFFECT_PARAM_NOT_THREADED
    #[test]
    fn effect_param_missing_from_caller_fails() {
        use ail_core::semantic_graph::EffectArgBinding;

        let mut caller = make_node(0, NodeKind::Function, "pure_caller");
        caller.effect_row = Some(EffectRow { effects: vec![] }); // empty

        let callee = make_node(1, NodeKind::Function, "effect_fn");

        let mut edge = make_edge(0, 1);
        edge.effect_arg_bindings = Some(vec![EffectArgBinding {
            param: "e".to_string(),
            effects: vec!["IO".to_string()],
        }]);

        let graph = SemanticGraph {
            nodes: vec![caller, callee],
            edges: vec![edge],
        };

        let report = TypeChecker::check(&graph);
        let threading = entries_with_claim(&report.entries, "effect-param-threading");
        let failed = threading.iter().find(|e| e.state == VerificationState::Failed);
        assert!(
            failed.is_some(),
            "effect missing from caller must be Failed, got: {:?}",
            threading
        );
        let ev = failed.unwrap().evidence.as_deref().unwrap_or("");
        assert!(
            ev.contains(E_EFFECT_PARAM_NOT_THREADED),
            "evidence must contain {E_EFFECT_PARAM_NOT_THREADED}, got: {ev}"
        );
    }

    // C3-3: CapabilityParam cap MISSING from caller → Failed.
    // Spec scenario: "CapabilityParam cap missing from caller"
    //   GIVEN caller with capability_reqs={caps:[]}
    //   AND Calls edge capability_arg_bindings=[{param:"cap", caps:["net:read"]}]
    //   THEN entry claim "capability-param-threading", state Failed, evidence E_CAPABILITY_PARAM_NOT_THREADED
    #[test]
    fn capability_param_missing_from_caller_fails() {
        use ail_core::semantic_graph::CapabilityArgBinding;

        let mut caller = make_node(0, NodeKind::Function, "no_cap_caller");
        caller.capability_reqs = Some(CapabilityReqs { caps: vec![] }); // empty

        let callee = make_node(1, NodeKind::Function, "cap_fn");

        let mut edge = make_edge(0, 1);
        edge.capability_arg_bindings = Some(vec![CapabilityArgBinding {
            param: "cap".to_string(),
            caps: vec!["net:read".to_string()],
        }]);

        let graph = SemanticGraph {
            nodes: vec![caller, callee],
            edges: vec![edge],
        };

        let report = TypeChecker::check(&graph);
        let threading = entries_with_claim(&report.entries, "capability-param-threading");
        let failed = threading.iter().find(|e| e.state == VerificationState::Failed);
        assert!(
            failed.is_some(),
            "cap missing from caller must be Failed, got: {:?}",
            threading
        );
        let ev = failed.unwrap().evidence.as_deref().unwrap_or("");
        assert!(
            ev.contains(E_CAPABILITY_PARAM_NOT_THREADED),
            "evidence must contain {E_CAPABILITY_PARAM_NOT_THREADED}, got: {ev}"
        );
    }

    // C3-4 (TRIANGULATE): CapabilityParam cap PRESENT in caller → Proven.
    #[test]
    fn capability_param_present_in_caller_is_proven() {
        use ail_core::semantic_graph::CapabilityArgBinding;

        let mut caller = make_node(0, NodeKind::Function, "cap_caller");
        caller.capability_reqs = Some(CapabilityReqs {
            caps: vec!["net:read".to_string()],
        });

        let callee = make_node(1, NodeKind::Function, "cap_fn");

        let mut edge = make_edge(0, 1);
        edge.capability_arg_bindings = Some(vec![CapabilityArgBinding {
            param: "cap".to_string(),
            caps: vec!["net:read".to_string()],
        }]);

        let graph = SemanticGraph {
            nodes: vec![caller, callee],
            edges: vec![edge],
        };

        let report = TypeChecker::check(&graph);
        let threading = entries_with_claim(&report.entries, "capability-param-threading");
        let proven = threading.iter().find(|e| e.state == VerificationState::Proven);
        assert!(
            proven.is_some(),
            "cap present in caller must be Proven, got: {:?}",
            threading
        );
    }

    // ── Task F3 (RED): check_boundary_schema ─────────────────────────────
    // Tests written BEFORE the subpass exists.

    // F3-1: ForeignType return without boundary-schema fact → Failed.
    // Spec scenario: "ForeignType without schema fails"
    //   GIVEN Function node return_type="ForeignType(payments.external)"
    //   AND no inferred fact of kind "boundary-schema"
    //   THEN entry claim "boundary-schema", state Failed, evidence E_FOREIGN_TYPE_NO_SCHEMA
    #[test]
    fn foreign_type_without_schema_fails() {
        let mut fn_node = make_node(0, NodeKind::Function, "fetch_payment");
        fn_node.return_type = Some("ForeignType(payments.external)".to_string());
        // no inferred facts

        let graph = SemanticGraph {
            nodes: vec![fn_node],
            edges: vec![],
        };

        let report = TypeChecker::check(&graph);
        let schema_entries = entries_with_claim(&report.entries, "boundary-schema");
        assert!(
            !schema_entries.is_empty(),
            "expected boundary-schema entry for ForeignType without schema"
        );
        let failed = schema_entries
            .iter()
            .find(|e| e.state == VerificationState::Failed);
        assert!(
            failed.is_some(),
            "ForeignType without boundary-schema must be Failed, got: {:?}",
            schema_entries
        );
        let ev = failed.unwrap().evidence.as_deref().unwrap_or("");
        assert!(
            ev.contains(E_FOREIGN_TYPE_NO_SCHEMA),
            "evidence must contain {E_FOREIGN_TYPE_NO_SCHEMA}, got: {ev}"
        );
    }

    // F3-2: ForeignType return WITH boundary-schema inferred fact → Proven.
    // Spec scenario: "ForeignType with schema passes"
    //   GIVEN Function node return_type="ForeignType(payments.external)"
    //   AND inferred fact { kind: "boundary-schema", value: "PaymentsJsonSchema" }
    //   THEN entry claim "boundary-schema", state Proven
    #[test]
    fn foreign_type_with_schema_fact_is_proven() {
        use ail_core::semantic_graph::InferredFact;

        let mut fn_node = make_node(0, NodeKind::Function, "fetch_payment");
        fn_node.return_type = Some("ForeignType(payments.external)".to_string());
        fn_node.inferred = vec![InferredFact {
            kind: "boundary-schema".to_string(),
            value: "PaymentsJsonSchema".to_string(),
        }];

        let graph = SemanticGraph {
            nodes: vec![fn_node],
            edges: vec![],
        };

        let report = TypeChecker::check(&graph);
        let schema_entries = entries_with_claim(&report.entries, "boundary-schema");
        let proven = schema_entries
            .iter()
            .find(|e| e.state == VerificationState::Proven);
        assert!(
            proven.is_some(),
            "ForeignType with boundary-schema fact must be Proven, got: {:?}",
            schema_entries
        );
    }

    // F3-3 (TRIANGULATE): Non-ForeignType return type must NOT emit boundary-schema entry.
    #[test]
    fn non_foreign_return_type_emits_no_boundary_schema_entry() {
        let mut fn_node = make_node(0, NodeKind::Function, "get_count");
        fn_node.return_type = Some("Int".to_string()); // not ForeignType

        let graph = SemanticGraph {
            nodes: vec![fn_node],
            edges: vec![],
        };

        let report = TypeChecker::check(&graph);
        let schema_entries = entries_with_claim(&report.entries, "boundary-schema");
        assert!(
            schema_entries.is_empty(),
            "non-ForeignType return must emit no boundary-schema entries, got: {:?}",
            schema_entries
        );
    }

    // ── Task D1 (RED): check_blanket_impl_coherence ───────────────────────
    // Tests written BEFORE the subpass exists.

    // D1-1: Two non-adapter Type nodes with same interface → E_BLANKET_IMPL_OVERLAP.
    // Spec scenario: "Two conflicting blanket impls detected"
    //   GIVEN two Type nodes both with interface_impls containing "Serializable<List<T>>" (non-adapter)
    //   THEN entry claim "blanket-impl-coherence", state Failed, evidence E_BLANKET_IMPL_OVERLAP
    #[test]
    fn two_non_adapter_impls_for_same_interface_fails() {
        let mut type_a = make_node(0, NodeKind::Type, "ConcreteListA");
        type_a.interface_impls = Some(vec![InterfaceImplMeta {
            interface: "Serializable<List<T>>".to_string(),
            associated_types: vec![],
            is_adapter: false,
        }]);

        let mut type_b = make_node(1, NodeKind::Type, "ConcreteListB");
        type_b.interface_impls = Some(vec![InterfaceImplMeta {
            interface: "Serializable<List<T>>".to_string(),
            associated_types: vec![],
            is_adapter: false,
        }]);

        let graph = SemanticGraph {
            nodes: vec![type_a, type_b],
            edges: vec![],
        };

        let report = TypeChecker::check(&graph);
        let blanket = entries_with_claim(&report.entries, "blanket-impl-coherence");
        assert!(
            !blanket.is_empty(),
            "expected blanket-impl-coherence entry for overlapping impls"
        );
        let failed = blanket
            .iter()
            .find(|e| e.state == VerificationState::Failed);
        assert!(
            failed.is_some(),
            "overlapping non-adapter impls must fail, got: {:?}",
            blanket
        );
        let ev = failed.unwrap().evidence.as_deref().unwrap_or("");
        assert!(
            ev.contains(E_BLANKET_IMPL_OVERLAP),
            "evidence must contain {E_BLANKET_IMPL_OVERLAP}, got: {ev}"
        );
    }

    // D1-2: Adapter impls are exempt from overlap rule.
    // Spec scenario: "Adapter impls are exempt from overlap rule"
    //   GIVEN two Type nodes with same interface_impls but both is_adapter=true
    //   THEN no E_BLANKET_IMPL_OVERLAP entry emitted
    #[test]
    fn adapter_impls_exempt_from_blanket_impl_overlap() {
        let mut type_a = make_node(0, NodeKind::Type, "AdapterA");
        type_a.interface_impls = Some(vec![InterfaceImplMeta {
            interface: "Serializable<List<T>>".to_string(),
            associated_types: vec![],
            is_adapter: true, // adapter exception
        }]);

        let mut type_b = make_node(1, NodeKind::Type, "AdapterB");
        type_b.interface_impls = Some(vec![InterfaceImplMeta {
            interface: "Serializable<List<T>>".to_string(),
            associated_types: vec![],
            is_adapter: true,
        }]);

        let graph = SemanticGraph {
            nodes: vec![type_a, type_b],
            edges: vec![],
        };

        let report = TypeChecker::check(&graph);
        let blanket = entries_with_claim(&report.entries, "blanket-impl-coherence");
        let overlap = blanket
            .iter()
            .find(|e| e.evidence.as_deref().unwrap_or("").contains(E_BLANKET_IMPL_OVERLAP));
        assert!(
            overlap.is_none(),
            "adapter impls must NOT trigger E_BLANKET_IMPL_OVERLAP, got: {:?}",
            blanket
        );
    }

    // D1-3: Orphan rule — Type with foreign interface impl and no Interface/Type node.
    // Spec scenario: "Orphan impl detected"
    //   GIVEN Type node "ExternalType" with interface_impls = [{ interface: "ForeignInterface", is_adapter: false }]
    //   AND no Interface node for "ForeignInterface" in graph
    //   THEN entry claim "orphan-rule", state Failed, evidence E_ORPHAN_RULE_VIOLATION
    #[test]
    fn orphan_impl_without_interface_node_fails() {
        let mut type_node = make_node(0, NodeKind::Type, "ExternalType");
        type_node.interface_impls = Some(vec![InterfaceImplMeta {
            interface: "ForeignInterface".to_string(),
            associated_types: vec![],
            is_adapter: false,
        }]);
        // No Interface node for "ForeignInterface" in the graph

        let graph = SemanticGraph {
            nodes: vec![type_node],
            edges: vec![],
        };

        let report = TypeChecker::check(&graph);
        let orphan = entries_with_claim(&report.entries, "orphan-rule");
        assert!(
            !orphan.is_empty(),
            "expected orphan-rule entry for impl without Interface node"
        );
        let failed = orphan
            .iter()
            .find(|e| e.state == VerificationState::Failed);
        assert!(
            failed.is_some(),
            "orphan impl must be Failed, got: {:?}",
            orphan
        );
        let ev = failed.unwrap().evidence.as_deref().unwrap_or("");
        assert!(
            ev.contains(E_ORPHAN_RULE_VIOLATION),
            "evidence must contain {E_ORPHAN_RULE_VIOLATION}, got: {ev}"
        );
    }

    // D1-4 (TRIANGULATE): Impl with Interface node present → no orphan violation.
    #[test]
    fn impl_with_local_interface_node_passes_orphan_rule() {
        let iface_node = make_node(0, NodeKind::Interface, "LocalInterface");

        let mut type_node = make_node(1, NodeKind::Type, "LocalType");
        type_node.interface_impls = Some(vec![InterfaceImplMeta {
            interface: "LocalInterface".to_string(),
            associated_types: vec![],
            is_adapter: false,
        }]);

        let graph = SemanticGraph {
            nodes: vec![iface_node, type_node],
            edges: vec![],
        };

        let report = TypeChecker::check(&graph);
        let orphan = entries_with_claim(&report.entries, "orphan-rule");
        let violation = orphan
            .iter()
            .find(|e| e.state == VerificationState::Failed);
        assert!(
            violation.is_none(),
            "impl with local Interface node must NOT trigger orphan-rule violation, got: {:?}",
            orphan
        );
    }

    // ── Task E1 (RED): ConstParam value validation ────────────────────────
    // Tests written BEFORE the extension exists — verifying current behavior
    // does NOT yet handle ConstParam.

    // E1-1: Valid ConstParam value "3" passes.
    // Spec scenario: "Valid ConstParam value passes"
    //   GIVEN Calls edge type_arg_bindings=[{param:"N",ty:"3"}]
    //   AND callee has ConstParam "N"
    //   THEN entry claim "const-param-value", state Proven
    #[test]
    fn const_param_numeric_value_is_proven() {
        let mut callee = make_node(1, NodeKind::Function, "buffer_fn");
        callee.generic_params = Some(vec![GenericParamDecl {
            name: "N".to_string(),
            kind: GenericParamKind::ConstParam,
            required_constraints: vec![],
        }]);

        let caller = make_node(0, NodeKind::Function, "caller");

        let mut edge = make_edge(0, 1);
        edge.type_arg_bindings = Some(vec![TypeArgBinding {
            param: "N".to_string(),
            ty: "3".to_string(),
        }]);

        let graph = SemanticGraph {
            nodes: vec![caller, callee],
            edges: vec![edge],
        };

        let report = TypeChecker::check(&graph);
        let const_entries = entries_with_claim(&report.entries, "const-param-value");
        assert!(
            !const_entries.is_empty(),
            "expected const-param-value entry for numeric literal, got none"
        );
        let proven = const_entries
            .iter()
            .find(|e| e.state == VerificationState::Proven);
        assert!(
            proven.is_some(),
            "numeric ConstParam value '3' must be Proven, got: {:?}",
            const_entries
        );
    }

    // E1-2 (TRIANGULATE): Valid ConstParam value "16" also passes.
    #[test]
    fn const_param_larger_numeric_value_is_proven() {
        let mut callee = make_node(1, NodeKind::Function, "vector_fn");
        callee.generic_params = Some(vec![GenericParamDecl {
            name: "N".to_string(),
            kind: GenericParamKind::ConstParam,
            required_constraints: vec![],
        }]);

        let caller = make_node(0, NodeKind::Function, "caller");

        let mut edge = make_edge(0, 1);
        edge.type_arg_bindings = Some(vec![TypeArgBinding {
            param: "N".to_string(),
            ty: "16".to_string(),
        }]);

        let graph = SemanticGraph {
            nodes: vec![caller, callee],
            edges: vec![edge],
        };

        let report = TypeChecker::check(&graph);
        let const_entries = entries_with_claim(&report.entries, "const-param-value");
        let proven = const_entries
            .iter()
            .find(|e| e.state == VerificationState::Proven);
        assert!(
            proven.is_some(),
            "numeric ConstParam value '16' must be Proven"
        );
    }

    // E1-3: Invalid ConstParam value "sizeof(T)" fails.
    // Spec scenario: "Invalid ConstParam value fails"
    //   GIVEN Calls edge type_arg_bindings=[{param:"N",ty:"sizeof(T)"}]
    //   AND callee has ConstParam "N"
    //   THEN entry claim "const-param-value", state Failed, evidence E_CONST_PARAM_VALUE_INVALID
    #[test]
    fn const_param_complex_expression_fails() {
        let mut callee = make_node(1, NodeKind::Function, "array_fn");
        callee.generic_params = Some(vec![GenericParamDecl {
            name: "N".to_string(),
            kind: GenericParamKind::ConstParam,
            required_constraints: vec![],
        }]);

        let caller = make_node(0, NodeKind::Function, "caller");

        let mut edge = make_edge(0, 1);
        edge.type_arg_bindings = Some(vec![TypeArgBinding {
            param: "N".to_string(),
            ty: "sizeof(T)".to_string(),
        }]);

        let graph = SemanticGraph {
            nodes: vec![caller, callee],
            edges: vec![edge],
        };

        let report = TypeChecker::check(&graph);
        let const_entries = entries_with_claim(&report.entries, "const-param-value");
        assert!(
            !const_entries.is_empty(),
            "expected const-param-value entry for invalid expression"
        );
        let failed = const_entries
            .iter()
            .find(|e| e.state == VerificationState::Failed);
        assert!(
            failed.is_some(),
            "sizeof(T) must be Failed, got: {:?}",
            const_entries
        );
        let ev = failed.unwrap().evidence.as_deref().unwrap_or("");
        assert!(
            ev.contains(E_CONST_PARAM_VALUE_INVALID),
            "evidence must contain {E_CONST_PARAM_VALUE_INVALID}, got: {ev}"
        );
    }

    // E1-4 (TRIANGULATE): "n+1" is also an invalid ConstParam value.
    #[test]
    fn const_param_arithmetic_expression_fails() {
        let mut callee = make_node(1, NodeKind::Function, "chunk_fn");
        callee.generic_params = Some(vec![GenericParamDecl {
            name: "N".to_string(),
            kind: GenericParamKind::ConstParam,
            required_constraints: vec![],
        }]);

        let caller = make_node(0, NodeKind::Function, "caller");

        let mut edge = make_edge(0, 1);
        edge.type_arg_bindings = Some(vec![TypeArgBinding {
            param: "N".to_string(),
            ty: "n+1".to_string(),
        }]);

        let graph = SemanticGraph {
            nodes: vec![caller, callee],
            edges: vec![edge],
        };

        let report = TypeChecker::check(&graph);
        let const_entries = entries_with_claim(&report.entries, "const-param-value");
        let failed = const_entries
            .iter()
            .find(|e| e.state == VerificationState::Failed);
        assert!(
            failed.is_some(),
            "n+1 must be Failed as invalid ConstParam value"
        );
    }

    // ── Task G1 (RED): Local type inference pre-pass ──────────────────────
    //
    // Tests written BEFORE infer_local_types exists.
    // Compile failure = RED state.

    // G1-1: Int literal body_expr suppresses E_BOUNDARY_NOT_MATERIALIZED.
    //
    // Spec scenario: "Int literal body → boundary Proven"
    //   GIVEN Function node with non-empty params, no return_type,
    //   AND body_expr = "42"
    //   THEN boundary-materialization entry state == Proven
    //   (because inferred return type is "Int")
    #[test]
    fn int_literal_body_expr_makes_boundary_proven() {
        use ail_core::semantic_graph::ParamDecl;

        let mut fn_node = make_node(0, NodeKind::Function, "answer");
        fn_node.params = Some(vec![ParamDecl {
            name: "x".to_string(),
            ty: "Int".to_string(),
        }]);
        fn_node.body_expr = Some("42".to_string()); // Int literal → infer "Int"
        // no return_type

        let graph = SemanticGraph {
            nodes: vec![fn_node],
            edges: vec![],
        };

        let report = TypeChecker::check(&graph);
        let boundary = entries_with_claim(&report.entries, "boundary-materialization");
        assert!(!boundary.is_empty(), "expected boundary-materialization entry");

        // Without inference: would be Unverified (E_BOUNDARY_NOT_MATERIALIZED).
        // With inference: should be Proven because "42" → "Int".
        let proven = boundary.iter().find(|e| e.state == VerificationState::Proven);
        assert!(
            proven.is_some(),
            "Int literal body_expr must make boundary-materialization Proven, got: {:?}",
            boundary
        );
    }

    // G1-2 (TRIANGULATE): Bool literal body_expr also infers correctly.
    //
    // Spec scenario: "Bool literal body → boundary Proven"
    #[test]
    fn bool_literal_body_expr_makes_boundary_proven() {
        use ail_core::semantic_graph::ParamDecl;

        let mut fn_node = make_node(0, NodeKind::Function, "is_valid");
        fn_node.params = Some(vec![ParamDecl {
            name: "val".to_string(),
            ty: "Text".to_string(),
        }]);
        fn_node.body_expr = Some("true".to_string());

        let graph = SemanticGraph {
            nodes: vec![fn_node],
            edges: vec![],
        };

        let report = TypeChecker::check(&graph);
        let boundary = entries_with_claim(&report.entries, "boundary-materialization");
        let proven = boundary.iter().find(|e| e.state == VerificationState::Proven);
        assert!(
            proven.is_some(),
            "Bool literal body_expr must make boundary-materialization Proven, got: {:?}",
            boundary
        );
    }

    // G1-3 (TRIANGULATE): No body_expr → still Unverified.
    //
    // Spec scenario: "No body_expr → boundary Unverified"
    //   Inference is only possible when body_expr is present.
    #[test]
    fn no_body_expr_keeps_boundary_unverified() {
        use ail_core::semantic_graph::ParamDecl;

        let mut fn_node = make_node(0, NodeKind::Function, "missing_body");
        fn_node.params = Some(vec![ParamDecl {
            name: "x".to_string(),
            ty: "Int".to_string(),
        }]);
        // no body_expr, no return_type

        let graph = SemanticGraph {
            nodes: vec![fn_node],
            edges: vec![],
        };

        let report = TypeChecker::check(&graph);
        let boundary = entries_with_claim(&report.entries, "boundary-materialization");
        let unverified = boundary
            .iter()
            .find(|e| e.state == VerificationState::Unverified);
        assert!(
            unverified.is_some(),
            "missing body_expr must keep boundary-materialization Unverified, got: {:?}",
            boundary
        );
    }

    // G1-4: Call-to-known-function body_expr infers callee's return type.
    //
    // Spec scenario: "Call body → infer callee return type"
    //   GIVEN Function "caller" with no return_type, body_expr = "helper"
    //   AND Function "helper" with return_type = "Text"
    //   THEN caller's boundary-materialization is Proven
    #[test]
    fn call_body_expr_infers_callee_return_type() {
        use ail_core::semantic_graph::ParamDecl;

        let mut caller = make_node(0, NodeKind::Function, "caller");
        caller.params = Some(vec![ParamDecl {
            name: "x".to_string(),
            ty: "Int".to_string(),
        }]);
        caller.body_expr = Some("helper".to_string()); // call to known function

        let mut helper = make_node(1, NodeKind::Function, "helper");
        helper.return_type = Some("Text".to_string());

        let graph = SemanticGraph {
            nodes: vec![caller, helper],
            edges: vec![],
        };

        let report = TypeChecker::check(&graph);
        let boundary = entries_with_claim(&report.entries, "boundary-materialization");

        // Only caller has params + no return_type; helper already has return_type.
        let caller_boundary: Vec<_> = boundary
            .iter()
            .filter(|e| e.scope == "caller")
            .collect();
        assert!(!caller_boundary.is_empty(), "expected boundary entry for caller");

        let proven = caller_boundary
            .iter()
            .find(|e| e.state == VerificationState::Proven);
        assert!(
            proven.is_some(),
            "call body_expr pointing to known function must make boundary Proven, got: {:?}",
            caller_boundary
        );
    }

    // G1-5: Declared return_type always wins over inference.
    //
    // Spec scenario: "Declared type wins over inference"
    //   GIVEN Function with return_type = "Int" AND body_expr = "true"
    //   THEN boundary-materialization is Proven (not a conflict)
    #[test]
    fn declared_return_type_wins_over_inferred() {
        use ail_core::semantic_graph::ParamDecl;

        let mut fn_node = make_node(0, NodeKind::Function, "explicit");
        fn_node.params = Some(vec![ParamDecl {
            name: "x".to_string(),
            ty: "Int".to_string(),
        }]);
        fn_node.return_type = Some("Int".to_string()); // declared
        fn_node.body_expr = Some("true".to_string()); // would infer Bool

        let graph = SemanticGraph {
            nodes: vec![fn_node],
            edges: vec![],
        };

        let report = TypeChecker::check(&graph);
        let boundary = entries_with_claim(&report.entries, "boundary-materialization");
        let proven = boundary.iter().find(|e| e.state == VerificationState::Proven);
        assert!(
            proven.is_some(),
            "function with declared return_type must always be Proven, got: {:?}",
            boundary
        );
    }
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
