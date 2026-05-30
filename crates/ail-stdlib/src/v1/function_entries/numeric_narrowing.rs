use super::*;

pub(super) fn add_entries(reg: &mut StdlibRegistry) {
    // ── std.numeric narrowing functions ───────────────────────────────────
    //
    // Pre-loop entries so contract_clauses survive the dedup loop, which
    // always injects with contract_clauses: None for new entries.

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.numeric.narrow_to_i32".to_string()),
        module_path: "std::numeric".to_string(),
        name: "narrow_to_i32".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["Int32".to_string(), "ArithError".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Int (i64)".to_string()],
            ensures: vec![
                "Ok(v) when value fits in i32 range".to_string(),
                "Err on overflow or underflow".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.numeric.narrow_to_u32".to_string()),
        module_path: "std::numeric".to_string(),
        name: "narrow_to_u32".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["UInt32".to_string(), "ArithError".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Int (i64)".to_string()],
            ensures: vec![
                "Ok(v) when value fits in u32 range (0..=4294967295)".to_string(),
                "Err on negative values or overflow".to_string(),
            ],
        }),
    });
}
