use super::*;

pub(super) fn add_entries(reg: &mut StdlibRegistry) {
    // ── std.text functions ────────────────────────────────────────────────

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.trim".to_string()),
        module_path: "std::text".to_string(),
        name: "trim".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Text".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["valid UTF-8 input".to_string()],
            ensures: vec!["result is valid Text with no leading/trailing whitespace".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.split".to_string()),
        module_path: "std::text".to_string(),
        name: "split".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "List".to_string(),
            generics: vec!["Text".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["valid UTF-8 input".to_string()],
            ensures: vec!["result is valid List<Text>".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.join".to_string()),
        module_path: "std::text".to_string(),
        name: "join".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Text".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["valid UTF-8 inputs".to_string()],
            ensures: vec!["result is valid Text".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.length_graphemes".to_string()),
        module_path: "std::text".to_string(),
        name: "length_graphemes".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "UInt".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["valid UTF-8 input".to_string()],
            ensures: vec!["result >= 0".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.to_bytes".to_string()),
        module_path: "std::text".to_string(),
        name: "to_bytes".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Bytes".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: None,
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.from_bytes".to_string()),
        module_path: "std::text".to_string(),
        name: "from_bytes".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["Text".to_string(), "DecodeError".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![],
            ensures: vec![
                "Ok(Text) if bytes are valid UTF-8".to_string(),
                "Err(DecodeError) if bytes are invalid UTF-8".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.regex".to_string()),
        module_path: "std::text".to_string(),
        name: "regex".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Bool".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["pattern is a valid regex".to_string()],
            ensures: vec!["returns Bool indicating whether pattern matches the input".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.starts_with".to_string()),
        module_path: "std::text".to_string(),
        name: "starts_with".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Bool".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["both arguments are valid UTF-8".to_string()],
            ensures: vec![
                "returns true if and only if the first argument begins with the prefix".to_string(),
                "empty prefix always returns true".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.ends_with".to_string()),
        module_path: "std::text".to_string(),
        name: "ends_with".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Bool".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["both arguments are valid UTF-8".to_string()],
            ensures: vec![
                "returns true if and only if the first argument ends with the suffix".to_string(),
                "empty suffix always returns true".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.contains".to_string()),
        module_path: "std::text".to_string(),
        name: "contains".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Bool".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["both arguments are valid UTF-8".to_string()],
            ensures: vec![
                "returns true if and only if needle appears as a substring".to_string(),
                "empty needle always returns true".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.text.replace".to_string()),
        module_path: "std::text".to_string(),
        name: "replace".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Text".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["all arguments are valid UTF-8".to_string()],
            ensures: vec![
                "every non-overlapping occurrence of `from` is replaced with `to`".to_string(),
                "empty `from` returns the input unchanged".to_string(),
            ],
        }),
    });
}
