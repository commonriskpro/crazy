// ── ail-verify::type_refinements ─────────────────────────────────────────
//
// Refinement, policy, boundary, generic-param, and effect subpasses for the
// type checker.
//
// All subpasses are exposed through this module to preserve existing callers;
// implementation is grouped by the reason each subpass changes.

mod absence;
mod boundaries;
mod effects;
mod generics;
mod helpers;
mod refinements;

pub(crate) use absence::{
    check_float_policy, check_null_policy, check_partial_ord, check_patchfield,
};
pub(crate) use boundaries::{
    check_boundary_inference, check_boundary_materialization, check_boundary_schema,
    infer_local_types,
};
pub(crate) use effects::{
    check_effect_capability_param_threading, check_effect_capability_propagation,
};
pub(crate) use generics::{
    check_const_param_call_bindings, check_generic_call_bindings, check_generic_params,
};
pub(crate) use refinements::{check_associated_type_resolution, check_refinements};
