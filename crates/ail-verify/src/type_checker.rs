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
//
// # Module layout
//
// Subpass implementations are split across focused sibling modules:
// - `type_obligations`  — nominal, variance, coherence, constraint subpasses
// - `type_refinements`  — generic params, effects, boundaries, refinements, policies
// - `type_diagnostics`  — report assembly (`build_summary_counts`)

use std::collections::BTreeMap;

use ail_core::semantic_graph::{GraphNode, NodeRef, SemanticGraph};

use crate::report::{VerificationEntry, VerificationReport};
use crate::{type_diagnostics, type_obligations, type_refinements};

// Imported only for test sub-module via `use super::*`.
#[cfg(test)]
use crate::report::VerificationState;

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
pub(crate) const COLLECTION_CONSTRAINTS: &[(&str, bool, bool, bool)] = &[
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
pub(crate) struct TypeContext<'a> {
    /// Nodes indexed by `NodeRef`.
    pub(crate) by_ref: BTreeMap<NodeRef, &'a GraphNode>,
    /// Nodes indexed by name.
    pub(crate) by_name: BTreeMap<&'a str, NodeRef>,
}

impl<'a> TypeContext<'a> {
    pub(crate) fn collect(graph: &'a SemanticGraph) -> Self {
        let mut by_ref = BTreeMap::new();
        let mut by_name = BTreeMap::new();
        for node in &graph.nodes {
            by_ref.insert(node.id, node);
            by_name.insert(node.name.as_str(), node.id);
        }
        TypeContext { by_ref, by_name }
    }

    pub(crate) fn get_by_name(&self, name: &str) -> Option<&GraphNode> {
        self.by_name
            .get(name)
            .and_then(|id| self.by_ref.get(id))
            .copied()
    }
}

// ── TypeChecker ───────────────────────────────────────────────────────────

/// Pure, stateless type checker.
///
/// Runs eighteen ordered subpasses over a `&SemanticGraph` and aggregates
/// all `VerificationEntry` items into a single `VerificationReport`.
pub struct TypeChecker;

impl TypeChecker {
    /// Walk `graph` and run all type-checking subpasses.
    ///
    /// # Determinism
    ///
    /// Two calls with identical input produce identical output.
    pub fn check(graph: &SemanticGraph) -> VerificationReport {
        // Pre-pass: infer local return types for Function nodes without an
        // explicit `return_type`.  Results feed into boundary-materialization
        // so that inferred signatures suppress E_BOUNDARY_NOT_MATERIALIZED.
        let inferred_returns = type_refinements::infer_local_types(graph);

        let ctx = TypeContext::collect(graph);
        let mut entries: Vec<VerificationEntry> = Vec::new();

        // Subpass 1 — nominal presence (backward-compatible).
        type_obligations::check_nominal_presence(graph, &mut entries);

        // Subpass 2 — nominal call check.
        type_obligations::check_nominal_calls(graph, &ctx, &mut entries);

        // Subpass 3 — generic param kind validation.
        type_refinements::check_generic_params(graph, &mut entries);

        // Subpass 3b — call-site generic arity/binding validation.
        type_refinements::check_generic_call_bindings(graph, &ctx, &mut entries);

        // Subpass 3c — effects and capabilities must propagate across calls.
        type_refinements::check_effect_capability_propagation(graph, &ctx, &mut entries);

        // Subpass 4 — variance enforcement.
        type_obligations::check_variance(graph, &ctx, &mut entries);

        // Subpass 4b — structural/Dyn interface call-site checks.
        type_obligations::check_structural_and_dyn_calls(graph, &ctx, &mut entries);

        // Subpass 5 — interface coherence.
        type_obligations::check_interface_coherence(graph, &mut entries);

        // Subpass 6 — constraint enforcement.
        type_obligations::check_constraints(graph, &ctx, &mut entries);

        // Subpass 7 — refinement proof obligations.
        type_refinements::check_refinements(graph, &mut entries);

        // Subpass 8 — boundary/inference materialization.
        type_refinements::check_boundary_materialization(graph, &inferred_returns, &mut entries);

        // Subpass 9 — null/absence policy.
        type_refinements::check_null_policy(graph, &mut entries);

        // Subpass 10 — float equality/ordering policy.
        type_refinements::check_float_policy(graph, &mut entries);

        // Subpass 11 — PatchField inner-type validation.
        type_refinements::check_patchfield(graph, &mut entries);

        // Subpass 12 — PartialOrd vs Ord distinction.
        type_refinements::check_partial_ord(graph, &mut entries);

        // Subpass 13 — Boundary inference cross-check.
        type_refinements::check_boundary_inference(graph, &mut entries);

        // Subpass 14 — Associated type resolution at call sites.
        type_refinements::check_associated_type_resolution(graph, &ctx, &mut entries);

        // Subpass 15 — ConstParam value validation at call sites (extends Subpass 3b).
        type_refinements::check_const_param_call_bindings(graph, &ctx, &mut entries);

        // Subpass 16 — ForeignType boundary schema enforcement.
        type_refinements::check_boundary_schema(graph, &mut entries);

        // Subpass 18 — Effect and capability parameter threading at call sites.
        type_refinements::check_effect_capability_param_threading(graph, &ctx, &mut entries);

        // Subpass 17 — Blanket impl coherence and orphan rule.
        type_obligations::check_blanket_impl_coherence(graph, &ctx, &mut entries);

        let summary_counts = type_diagnostics::build_summary_counts(&entries);

        VerificationReport {
            entries,
            schema_version: "verification/1.0".into(),
            summary_counts,
            ..Default::default()
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ail_core::semantic_graph::{
        AssociatedTypeBinding, CapabilityReqs, EdgeKind, EffectRow, GenericParamDecl,
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
        let proven = assoc.iter().find(|e| e.state == VerificationState::Proven);
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
        use ail_core::semantic_graph::EffectArgBinding;

        let mut caller = make_node(0, NodeKind::Function, "caller_fn");
        caller.effect_row = Some(EffectRow {
            effects: vec!["IO".to_string()],
        });

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
        let proven = threading
            .iter()
            .find(|e| e.state == VerificationState::Proven);
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
        let failed = threading
            .iter()
            .find(|e| e.state == VerificationState::Failed);
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
        let failed = threading
            .iter()
            .find(|e| e.state == VerificationState::Failed);
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
        let proven = threading
            .iter()
            .find(|e| e.state == VerificationState::Proven);
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
        let overlap = blanket.iter().find(|e| {
            e.evidence
                .as_deref()
                .unwrap_or("")
                .contains(E_BLANKET_IMPL_OVERLAP)
        });
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
        let failed = orphan.iter().find(|e| e.state == VerificationState::Failed);
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
        let violation = orphan.iter().find(|e| e.state == VerificationState::Failed);
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
        assert!(
            !boundary.is_empty(),
            "expected boundary-materialization entry"
        );

        // Without inference: would be Unverified (E_BOUNDARY_NOT_MATERIALIZED).
        // With inference: should be Proven because "42" → "Int".
        let proven = boundary
            .iter()
            .find(|e| e.state == VerificationState::Proven);
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
        let proven = boundary
            .iter()
            .find(|e| e.state == VerificationState::Proven);
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
        let caller_boundary: Vec<_> = boundary.iter().filter(|e| e.scope == "caller").collect();
        assert!(
            !caller_boundary.is_empty(),
            "expected boundary entry for caller"
        );

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
        let proven = boundary
            .iter()
            .find(|e| e.state == VerificationState::Proven);
        assert!(
            proven.is_some(),
            "function with declared return_type must always be Proven, got: {:?}",
            boundary
        );
    }
}
