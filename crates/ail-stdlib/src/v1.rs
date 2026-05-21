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

use ail_core::semantic_graph::NodeKind;

use crate::registry::{StabilityTier, StdlibEntry, StdlibId, StdlibRegistry};

/// Return the canonical v1 stdlib registry.
///
/// Contains exactly 9 ordered entries corresponding to the v1 semantic-core
/// scope.  All entries carry `StabilityTier::Stable` and `NodeKind::Module`.
/// Optional semantic-fact fields are `None` — they are populated by the
/// compiler/verify layer, not by the data registry.
///
/// The returned `StdlibRegistry` is guaranteed to pass `validate()`.
pub fn v1_registry() -> StdlibRegistry {
    StdlibRegistry {
        entries: vec![
            // 0 — std.core
            StdlibEntry {
                id: StdlibId("std.core".to_string()),
                module_path: "std::core".to_string(),
                name: "core".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: None,
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
            // 1 — std.option
            StdlibEntry {
                id: StdlibId("std.option".to_string()),
                module_path: "std::option".to_string(),
                name: "option".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: None,
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
            // 2 — std.result
            StdlibEntry {
                id: StdlibId("std.result".to_string()),
                module_path: "std::result".to_string(),
                name: "result".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: None,
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
            // 3 — std.numeric
            StdlibEntry {
                id: StdlibId("std.numeric".to_string()),
                module_path: "std::numeric".to_string(),
                name: "numeric".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: None,
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
            // 4 — std.text
            StdlibEntry {
                id: StdlibId("std.text".to_string()),
                module_path: "std::text".to_string(),
                name: "text".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: None,
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
            // 5 — std.bytes
            StdlibEntry {
                id: StdlibId("std.bytes".to_string()),
                module_path: "std::bytes".to_string(),
                name: "bytes".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: None,
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
            // 6 — std.collections
            StdlibEntry {
                id: StdlibId("std.collections".to_string()),
                module_path: "std::collections".to_string(),
                name: "collections".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: None,
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
            // 7 — std.iter
            StdlibEntry {
                id: StdlibId("std.iter".to_string()),
                module_path: "std::iter".to_string(),
                name: "iter".to_string(),
                kind: NodeKind::Module,
                stability: StabilityTier::Stable,
                type_facts: None,
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
            // 8 — std.capability
            StdlibEntry {
                id: StdlibId("std.capability".to_string()),
                module_path: "std::capability".to_string(),
                name: "capability".to_string(),
                kind: NodeKind::Capability,
                stability: StabilityTier::Stable,
                type_facts: None,
                effect_row: None,
                capability_reqs: None,
                contract_clauses: None,
            },
        ],
    }
}
