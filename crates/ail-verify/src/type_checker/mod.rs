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
//    - If a Type node's `type_facts.nominal == "Float" AND has_eq=true
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

mod checker;
mod codes;
mod context;

pub use checker::TypeChecker;
pub use codes::*;
pub(crate) use context::TypeContext;

#[cfg(test)]
mod tests;
