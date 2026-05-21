// ── ail-stdlib::v1 ────────────────────────────────────────────────────────
//
// Canonical v1 stdlib module registry.
//
// # v1 module gate
//
// Exactly 9 modules are approved for the semantic-core v1 scope:
//
//   std.core, std.option, std.result, std.numeric, std.text,
//   std.bytes, std.collections, std.iter, std.capability
//
// No other module is present.  Future modules require a v2 gate or an
// explicit extension of this function under a new name.
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

use ail_core::semantic_graph::{CapabilityReqs, ContractClauses, EffectRow, NodeKind, TypeFacts};

use crate::capability;
use crate::registry::{StabilityTier, StdlibEntry, StdlibId, StdlibRegistry};

/// Return the canonical v1 stdlib registry.
///
/// Contains exactly 9 ordered entries corresponding to the v1 semantic-core
/// scope.  All entries carry `StabilityTier::Stable`.  Semantic-fact fields
/// are populated to reflect the type, effect, capability, and contract
/// metadata documented in `docs/stdlib.md`.
///
/// The returned `StdlibRegistry` is guaranteed to pass `validate()`.
pub fn v1_registry() -> StdlibRegistry {
    StdlibRegistry {
        entries: vec![
            // 0 — std.core
            //
            // Base language helpers: Unit, Never, Bool, Ordering, Identity.
            // Interfaces: Eq, Hashable, Ord, PartialOrd, Debug, Display.
            // No effects, no capability requirements, no module-level contracts.
            StdlibEntry {
                id: StdlibId("std.core".to_string()),
                module_path: "std::core".to_string(),
                name: "core".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: Some(TypeFacts {
                    nominal: "Module".to_string(),
                    generics: vec![],
                }),
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
            // 1 — std.option
            //
            // Option<T> = Some(T) | None.
            // Generic over one type parameter T.
            StdlibEntry {
                id: StdlibId("std.option".to_string()),
                module_path: "std::option".to_string(),
                name: "option".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: Some(TypeFacts {
                    nominal: "Option".to_string(),
                    generics: vec!["T".to_string()],
                }),
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
            // 2 — std.result
            //
            // Result<T, E> = Ok(T) | Err(E).
            // Generic over two type parameters: value type T and error type E.
            StdlibEntry {
                id: StdlibId("std.result".to_string()),
                module_path: "std::result".to_string(),
                name: "result".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: Some(TypeFacts {
                    nominal: "Result".to_string(),
                    generics: vec!["T".to_string(), "E".to_string()],
                }),
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
            // 3 — std.numeric
            //
            // Int, UInt, Int32, Int64, UInt32, UInt64, Float, Decimal<Scale, Precision>.
            // Generic over numeric kind N (the numeric type parameter).
            // Contracts: no silent overflow, no silent narrowing.
            StdlibEntry {
                id: StdlibId("std.numeric".to_string()),
                module_path: "std::numeric".to_string(),
                name: "numeric".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: Some(TypeFacts {
                    nominal: "Numeric".to_string(),
                    generics: vec!["N".to_string()],
                }),
                effect_row: None,
                capability_reqs: None,
                contract_clauses: Some(ContractClauses {
                    requires: vec![
                        "no silent overflow".to_string(),
                        "no silent narrowing".to_string(),
                    ],
                    ensures: vec!["result is defined numeric type".to_string()],
                }),
            },
            // 4 — std.text
            //
            // Text, CodePoint, Grapheme, NormalizedText<Form>.
            // Refinements: NonEmptyText, Email, Url, Slug.
            // Contract: input must be valid UTF-8; result is always valid Text.
            StdlibEntry {
                id: StdlibId("std.text".to_string()),
                module_path: "std::text".to_string(),
                name: "text".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: Some(TypeFacts {
                    nominal: "Text".to_string(),
                    generics: vec![],
                }),
                effect_row: None,
                capability_reqs: None,
                contract_clauses: Some(ContractClauses {
                    requires: vec!["valid UTF-8 input".to_string()],
                    ensures: vec!["result is valid Text".to_string()],
                }),
            },
            // 5 — std.bytes
            //
            // Bytes — raw byte sequence.  No generics, no effects, no contracts.
            StdlibEntry {
                id: StdlibId("std.bytes".to_string()),
                module_path: "std::bytes".to_string(),
                name: "bytes".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: Some(TypeFacts {
                    nominal: "Bytes".to_string(),
                    generics: vec![],
                }),
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
            // 6 — std.collections
            //
            // List<T>, Set<T>, Map<K,V>, Vector<T,N>, OrderedSet<T>, OrderedMap<K,V>, Array<T>.
            // Generic over element type T.
            // Contracts: length >= 0; no duplicate keys in Map; ordering is explicit by type.
            StdlibEntry {
                id: StdlibId("std.collections".to_string()),
                module_path: "std::collections".to_string(),
                name: "collections".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: Some(TypeFacts {
                    nominal: "Collection".to_string(),
                    generics: vec!["T".to_string()],
                }),
                effect_row: None,
                capability_reqs: None,
                contract_clauses: Some(ContractClauses {
                    requires: vec!["length >= 0".to_string()],
                    ensures: vec![
                        "no duplicate keys in Map".to_string(),
                        "ordering is explicit by type".to_string(),
                    ],
                }),
            },
            // 7 — std.iter
            //
            // Effect-polymorphic iterator abstractions: map<T,U,e>, filter<T>,
            // fold<T,U,e>, traverse<T,U,E,e>.
            // Generic over element type T and effect parameter E.
            // EffectPoly: helpers preserve the effect parameter — they do not
            // hide or erase the caller's declared effects.
            StdlibEntry {
                id: StdlibId("std.iter".to_string()),
                module_path: "std::iter".to_string(),
                name: "iter".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: Some(TypeFacts {
                    nominal: "Iterator".to_string(),
                    generics: vec!["T".to_string(), "E".to_string()],
                }),
                effect_row: Some(EffectRow {
                    effects: vec!["EffectPoly".to_string()],
                }),
                capability_reqs: None,
                contract_clauses: None,
            },
            // 8 — std.capability
            //
            // Common capability abstractions: CapabilityId, CapabilityGrant,
            // CapabilityManifest, HandlerBinding, HostResult<T>, HostError.
            // This module IS the canonical definition of all common capabilities.
            // It declares — but does not grant — each capability it exports.
            StdlibEntry {
                id: StdlibId("std.capability".to_string()),
                module_path: "std::capability".to_string(),
                name: "capability".to_string(),
                kind: NodeKind::Capability,
                stability: StabilityTier::Stable,
                type_facts: Some(TypeFacts {
                    nominal: "CapabilitySet".to_string(),
                    generics: vec![],
                }),
                effect_row: Some(EffectRow {
                    effects: vec![
                        capability::CLOCK_NOW.to_string(),
                        capability::NET_CONNECT.to_string(),
                        capability::NET_BIND.to_string(),
                        capability::FS_READ.to_string(),
                        capability::FS_WRITE.to_string(),
                        capability::IO_STDIN.to_string(),
                        capability::IO_STDOUT.to_string(),
                        capability::IO_STDERR.to_string(),
                        capability::PROCESS_EXEC.to_string(),
                        capability::ENV_READ.to_string(),
                        capability::ENV_WRITE.to_string(),
                        capability::RANDOM_GENERATE.to_string(),
                        capability::LOG_EMIT.to_string(),
                        capability::TRACE_SPAN.to_string(),
                    ],
                }),
                capability_reqs: Some(CapabilityReqs {
                    caps: vec![
                        capability::CLOCK_NOW.to_string(),
                        capability::NET_CONNECT.to_string(),
                        capability::NET_BIND.to_string(),
                        capability::FS_READ.to_string(),
                        capability::FS_WRITE.to_string(),
                        capability::IO_STDIN.to_string(),
                        capability::IO_STDOUT.to_string(),
                        capability::IO_STDERR.to_string(),
                        capability::PROCESS_EXEC.to_string(),
                        capability::ENV_READ.to_string(),
                        capability::ENV_WRITE.to_string(),
                        capability::RANDOM_GENERATE.to_string(),
                        capability::LOG_EMIT.to_string(),
                        capability::TRACE_SPAN.to_string(),
                    ],
                }),
                contract_clauses: None,
            },
        ],
    }
}
