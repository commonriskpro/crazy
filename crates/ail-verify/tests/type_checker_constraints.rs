// ── ail-verify::type_checker — constraint and policy tests ────────────────
//
// Integration tests for TypeChecker:
//   Subpass 6  — Constraint enforcement (Eq/Hash/Ord requirements, generic fn
//                where-clauses, ConstParam decidability, pipeline compatibility)
//   Subpass 7  — Refinements (Proven, RuntimeChecked, erasure, Failed)
//   Subpass 8  — Boundary materialization
//   Subpass 9  — Null/absence policy
//   Subpass 10 — Float equality/ordering policy
//   Subpass 5′ — Improved associated-type binding validation
//   Task E1    — PatchField validation
//   Task E3    — PartialOrd validation
//   Task E5    — Boundary inference cross-check
//   Task F1    — Combined integration scenario

#[path = "type_checker_constraints/absence.rs"]
mod absence;
#[path = "type_checker_constraints/boundaries.rs"]
mod boundaries;
#[path = "type_checker_constraints/constraints.rs"]
mod constraints;
#[path = "type_checker_constraints/float.rs"]
mod float;
#[path = "type_checker_constraints/helpers.rs"]
mod helpers;
#[path = "type_checker_constraints/refinements.rs"]
mod refinements;
