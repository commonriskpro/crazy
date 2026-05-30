use super::*;

pub(super) fn add_entries(reg: &mut StdlibRegistry) {
    // ── std.time pure functions ───────────────────────────────────────────
    //
    // Pre-loop entries so contract_clauses survive the dedup loop.

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.time.duration_since".to_string()),
        module_path: "std::time".to_string(),
        name: "duration_since".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Int".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["both args are Int (millisecond epoch instants)".to_string()],
            ensures: vec![
                "result is (first - second) in milliseconds".to_string(),
                "result is negative when second instant is later than first".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.time.add_duration".to_string()),
        module_path: "std::time".to_string(),
        name: "add_duration".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Int".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is Int (millisecond epoch instant)".to_string(),
                "second arg is Int (duration in milliseconds)".to_string(),
            ],
            ensures: vec!["result is the sum of instant and duration in milliseconds".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.time.instant_to_ms".to_string()),
        module_path: "std::time".to_string(),
        name: "instant_to_ms".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Int".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["input is Int (epoch-millisecond instant)".to_string()],
            ensures: vec![
                "result is the same Int value (identity projection for epoch-ms instants)"
                    .to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.time.now".to_string()),
        module_path: "std::time".to_string(),
        name: "now".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Instant".to_string(),
            generics: vec![],
        }),
        effect_row: Some(EffectRow {
            effects: vec!["clock.now".to_string()],
        }),
        capability_reqs: Some(CapabilityReqs {
            caps: vec!["clock.now".to_string()],
        }),
        contract_clauses: Some(ContractClauses {
            requires: vec!["clock.now capability must be granted".to_string()],
            ensures: vec![
                "result is Instant (runtime representation: Int epoch-ms since Unix epoch)"
                    .to_string(),
                "result > 0 for any real-world wall-clock call".to_string(),
            ],
        }),
    });
}
