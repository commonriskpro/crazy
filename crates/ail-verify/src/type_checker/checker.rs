use ail_core::semantic_graph::SemanticGraph;

use crate::report::{VerificationEntry, VerificationReport};
use crate::{type_diagnostics, type_obligations, type_refinements};

use super::TypeContext;

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
