use super::*;

pub(super) fn add_entries(reg: &mut StdlibRegistry) {
    // ── std.bytes functions ───────────────────────────────────────────────
    //
    // Pre-loop entries so contract_clauses survive the dedup loop.

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.bytes.length".to_string()),
        module_path: "std::bytes".to_string(),
        name: "length".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Int".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Bytes".to_string()],
            ensures: vec![
                "result >= 0".to_string(),
                "result equals the number of bytes in the buffer".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.bytes.at".to_string()),
        module_path: "std::bytes".to_string(),
        name: "at".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["Int".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is Bytes".to_string(),
                "second arg is Int (index)".to_string(),
            ],
            ensures: vec![
                "Some(v) where v is in 0..=255 when 0 <= index < length".to_string(),
                "None when index is negative or >= length".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.bytes.slice".to_string()),
        module_path: "std::bytes".to_string(),
        name: "slice".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["Bytes".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is Bytes".to_string(),
                "second and third args are Int (start, end)".to_string(),
            ],
            ensures: vec![
                "Some(Bytes) containing [start..end] bytes when 0 <= start <= end <= length"
                    .to_string(),
                "None when start or end is negative, start > end, or end > length".to_string(),
                "Some(empty Bytes) when start == end and both are in bounds".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.bytes.concat".to_string()),
        module_path: "std::bytes".to_string(),
        name: "concat".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Bytes".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["both args are Bytes".to_string()],
            ensures: vec![
                "result contains all bytes of the first buffer followed by all bytes of the second"
                    .to_string(),
                "neither input is mutated".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.bytes.empty".to_string()),
        module_path: "std::bytes".to_string(),
        name: "empty".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Bool".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Bytes".to_string()],
            ensures: vec![
                "true when buffer has zero bytes".to_string(),
                "false when buffer has one or more bytes".to_string(),
            ],
        }),
    });
}
