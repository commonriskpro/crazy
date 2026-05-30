use super::*;

pub(super) fn add_entries(reg: &mut StdlibRegistry) {
    // ── std.iter functions ────────────────────────────────────────────────

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.iter.map".to_string()),
        module_path: "std::iter".to_string(),
        name: "map".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "List".to_string(),
            generics: vec!["T".to_string(), "U".to_string()],
        }),
        effect_row: Some(EffectRow {
            effects: vec!["EffectPoly".to_string()],
        }),
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "input is List<T>".to_string(),
                "f is a total function T -> U".to_string(),
            ],
            ensures: vec![
                "output length equals input length".to_string(),
                "output[i] = f(input[i]) for every i".to_string(),
                "empty input returns empty list".to_string(),
                "effects of f are preserved (EffectPoly)".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.iter.filter".to_string()),
        module_path: "std::iter".to_string(),
        name: "filter".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "List".to_string(),
            generics: vec!["T".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "input is List<T>".to_string(),
                "pred is a total predicate T -> Bool".to_string(),
            ],
            ensures: vec![
                "result is a subsequence of input".to_string(),
                "every retained element satisfies pred".to_string(),
                "relative order of retained elements is preserved".to_string(),
                "empty input returns empty list".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.iter.fold".to_string()),
        module_path: "std::iter".to_string(),
        name: "fold".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "U".to_string(),
            generics: vec!["T".to_string(), "U".to_string()],
        }),
        effect_row: Some(EffectRow {
            effects: vec!["EffectPoly".to_string()],
        }),
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "input is List<T>".to_string(),
                "init is U (accumulator seed)".to_string(),
                "f receives one List([acc, item]) binary-encoded pair (acc: U, item: T) and returns U".to_string(),
            ],
            ensures: vec![
                "empty input returns init unchanged".to_string(),
                "result is left fold of f over items starting from init".to_string(),
                "effects of f are preserved (EffectPoly)".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.iter.traverse".to_string()),
        module_path: "std::iter".to_string(),
        name: "traverse".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec![
                "List".to_string(),
                "T".to_string(),
                "U".to_string(),
                "E".to_string(),
            ],
        }),
        effect_row: Some(EffectRow {
            effects: vec!["EffectPoly".to_string()],
        }),
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "input is List<T>".to_string(),
                "f is a total function T -> Result<U, E>".to_string(),
            ],
            ensures: vec![
                "Ok(List<U>) when all applications of f succeed".to_string(),
                "Err(e) from the first failed application of f".to_string(),
                "short-circuits: no elements after the first Err are evaluated".to_string(),
                "effects of f are preserved (EffectPoly)".to_string(),
            ],
        }),
    });
}
