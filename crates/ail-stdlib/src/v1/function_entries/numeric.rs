use super::*;

pub(super) fn add_entries(reg: &mut StdlibRegistry) {
    // ── std.numeric functions ─────────────────────────────────────────────

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.numeric.checked_add".to_string()),
        module_path: "std::numeric".to_string(),
        name: "checked_add".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["i64".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["no silent overflow".to_string()],
            ensures: vec!["returns None on overflow".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.numeric.wrapping_add".to_string()),
        module_path: "std::numeric".to_string(),
        name: "wrapping_add".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "i64".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["wrapping semantics chosen explicitly".to_string()],
            ensures: vec!["result wraps on overflow (defined, not silent)".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.numeric.saturating_add".to_string()),
        module_path: "std::numeric".to_string(),
        name: "saturating_add".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "i64".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["saturating semantics chosen explicitly".to_string()],
            ensures: vec!["result clamped to i64::MAX or i64::MIN on overflow".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.numeric.checked_sub".to_string()),
        module_path: "std::numeric".to_string(),
        name: "checked_sub".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["i64".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["no silent underflow".to_string()],
            ensures: vec!["returns None on underflow or overflow".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.numeric.checked_mul".to_string()),
        module_path: "std::numeric".to_string(),
        name: "checked_mul".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["i64".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["no silent overflow".to_string()],
            ensures: vec!["returns None on overflow".to_string()],
        }),
    });
}
