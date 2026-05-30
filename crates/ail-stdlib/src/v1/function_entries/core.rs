use super::*;

pub(super) fn add_entries(reg: &mut StdlibRegistry) {
    // ── std.core.option functions ─────────────────────────────────────────
    //
    // IDs use the std.core.* namespace to match exec handler registration in
    // exec/registry.rs.  The dedup loop (below) skips entries already present,
    // so these pre-loop entries are what carry the contract_clauses.

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.option.map".to_string()),
        module_path: "std::core".to_string(),
        name: "map".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["T".to_string(), "U".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Option<T>".to_string()],
            ensures: vec![
                "None returns None without calling f".to_string(),
                "Some(v) returns Some(f(v))".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.option.and_then".to_string()),
        module_path: "std::core".to_string(),
        name: "and_then".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["T".to_string(), "U".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Option<T>".to_string()],
            ensures: vec![
                "None short-circuits without calling f".to_string(),
                "Some(v) returns f(v)".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.option.unwrap_or".to_string()),
        module_path: "std::core".to_string(),
        name: "unwrap_or".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "T".to_string(),
            generics: vec!["T".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Option<T>".to_string()],
            ensures: vec![
                "None returns the default value".to_string(),
                "Some(v) returns v".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.option.transpose".to_string()),
        module_path: "std::core".to_string(),
        name: "transpose".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["Option".to_string(), "T".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Option<Result<T, E>>".to_string()],
            ensures: vec![
                "Some(Ok(v)) -> Ok(Some(v))".to_string(),
                "Some(Err(e)) -> Err(e)".to_string(),
                "None -> Ok(None)".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.option.collect_results".to_string()),
        module_path: "std::core".to_string(),
        name: "collect_results".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["List".to_string(), "T".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is List<Result<T, E>>".to_string()],
            ensures: vec![
                "Ok(List<T>) when all items are Ok".to_string(),
                "Err(e) on the first Err encountered".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.option.ok_or".to_string()),
        module_path: "std::core".to_string(),
        name: "ok_or".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["T".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is Option<T>".to_string(),
                "second arg is the error value E".to_string(),
            ],
            ensures: vec![
                "Some(v) returns Ok(v)".to_string(),
                "None returns Err(err)".to_string(),
            ],
        }),
    });

    // ── std.core.result functions ─────────────────────────────────────────

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.result.map".to_string()),
        module_path: "std::core".to_string(),
        name: "map".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["T".to_string(), "U".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Result<T, E>".to_string()],
            ensures: vec![
                "Err(e) passes through unchanged without calling f".to_string(),
                "Ok(v) returns Ok(f(v))".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.result.and_then".to_string()),
        module_path: "std::core".to_string(),
        name: "and_then".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["T".to_string(), "U".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Result<T, E>".to_string()],
            ensures: vec![
                "Err(e) short-circuits without calling f".to_string(),
                "Ok(v) returns f(v)".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.result.unwrap_or".to_string()),
        module_path: "std::core".to_string(),
        name: "unwrap_or".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "T".to_string(),
            generics: vec!["T".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Result<T, E>".to_string()],
            ensures: vec![
                "Err returns the default value".to_string(),
                "Ok(v) returns v".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.core.result.transpose".to_string()),
        module_path: "std::core".to_string(),
        name: "transpose".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["Result".to_string(), "T".to_string(), "E".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Result<Option<T>, E>".to_string()],
            ensures: vec![
                "Ok(Some(v)) -> Some(Ok(v))".to_string(),
                "Ok(None) -> None".to_string(),
                "Err(e) -> Some(Err(e))".to_string(),
            ],
        }),
    });
}
