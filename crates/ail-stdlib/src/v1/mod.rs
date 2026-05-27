// ── ail-stdlib::v1 ────────────────────────────────────────────────────────
//
// Canonical v1 stdlib module registry.
//
// # v1 module gate
//
// The v1 registry contains all modules listed in docs/stdlib.md.
//
// # Entry order
//
// Declaration order is stable and determines `NodeRef` assignment during
// graph projection.  Do not reorder entries after the v1 registry ships.
//
// # Metadata conventions
//
// `type_facts.nominal` names the primary exported type (not the module).
// `effect_row` is populated for effect-bearing or effect-polymorphic modules.
// `capability_reqs` is populated only for `std.capability` (the definition module).
// `contract_clauses` carries module-level invariants from docs/stdlib.md.
//
// # G26: Function entries
//
// `v1_registry_with_functions()` extends the base module registry with
// `NodeKind::Function` entries for each semantic function implementation
// in the std.numeric, std.option, std.result, std.text, and std.iter modules.
// The base `v1_registry()` is preserved unchanged for backward compatibility.

mod function_entries;
mod module_entries;

pub use function_entries::v1_registry_with_functions;
pub use module_entries::v1_registry;

#[cfg(test)]
mod tests;
