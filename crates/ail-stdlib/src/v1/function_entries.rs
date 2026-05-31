use ail_core::semantic_graph::{CapabilityReqs, ContractClauses, EffectRow, NodeKind, TypeFacts};

use crate::exec::{FunctionImpl, stdlib_function_entries};
use crate::registry::{StabilityTier, StdlibEntry, StdlibId, StdlibRegistry};

use super::module_entries::v1_registry;

mod bytes;
mod collections;
mod core;
mod iter;
mod numeric;
mod numeric_narrowing;
mod text;
mod time;

/// Return the extended v1 stdlib registry with `NodeKind::Function` entries.
///
/// Starts from `v1_registry()` (the 9-module base) and appends one
/// `StdlibEntry` per implemented function in:
///
/// - `std.numeric`: `checked_add`, `wrapping_add`, `saturating_add`,
///   `checked_sub`, `checked_mul`
/// - `std.core.option`: `map`, `and_then`, `unwrap_or`, `transpose`,
///   `collect_results`
/// - `std.core.result`: `map`, `and_then`, `unwrap_or`, `transpose`
/// - `std.text`: `trim`, `split`, `join`, `length_graphemes`,
///   `to_bytes`, `from_bytes`, `starts_with`, `ends_with`, `contains`, `replace`
/// - `std.iter`: `map`, `filter`, `any`, `all`, `find`, `position`, `fold`,
///   `traverse`
/// - `std.collections`: list/map/set operations including length, membership,
///   lookup, insertion, and functional list adapters
///
/// The returned registry is guaranteed to pass `validate()`.
pub fn v1_registry_with_functions() -> StdlibRegistry {
    let mut reg = v1_registry();

    numeric::add_entries(&mut reg);
    core::add_entries(&mut reg);
    text::add_entries(&mut reg);
    iter::add_entries(&mut reg);
    numeric_narrowing::add_entries(&mut reg);
    bytes::add_entries(&mut reg);
    collections::add_entries(&mut reg);
    time::add_entries(&mut reg);
    append_registered_function_entries(&mut reg);

    reg
}

fn append_registered_function_entries(reg: &mut StdlibRegistry) {
    for function in stdlib_function_entries() {
        if reg.entries.iter().any(|entry| entry.id.0 == function.id) {
            continue;
        }

        let (effect_row, capability_reqs) = match function.implementation {
            FunctionImpl::Pure(_) => (None, None),
            FunctionImpl::Capability { capability, .. } => {
                let caps = vec![capability.to_string()];
                (
                    Some(EffectRow {
                        effects: caps.clone(),
                    }),
                    Some(CapabilityReqs { caps }),
                )
            }
        };

        reg.entries.push(StdlibEntry {
            id: StdlibId(function.id.to_string()),
            module_path: function.module.replace('.', "::"),
            name: function.name.to_string(),
            kind: NodeKind::Function,
            stability: StabilityTier::Stable,
            type_facts: Some(TypeFacts {
                nominal: function.return_type.to_string(),
                generics: function
                    .params
                    .iter()
                    .map(|param| (*param).to_string())
                    .collect(),
            }),
            effect_row,
            capability_reqs,
            contract_clauses: None,
        });
    }
}
