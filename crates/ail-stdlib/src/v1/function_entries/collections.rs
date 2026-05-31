use super::*;

pub(super) fn add_entries(reg: &mut StdlibRegistry) {
    // ── std.collections list functions ───────────────────────────────────
    //
    // Pre-loop entries so contract_clauses survive the dedup loop, which
    // always injects contract_clauses: None for new entries.

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.list.length".to_string()),
        module_path: "std::collections".to_string(),
        name: "length".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "UInt".to_string(),
            generics: vec!["T".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["first arg is List<T>".to_string()],
            ensures: vec![
                "result >= 0".to_string(),
                "result equals the number of elements in the list".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.list.is_empty".to_string()),
        module_path: "std::collections".to_string(),
        name: "is_empty".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Bool".to_string(),
            generics: vec!["T".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["first arg is List<T>".to_string()],
            ensures: vec![
                "true when list length is zero".to_string(),
                "false when list contains one or more elements".to_string(),
                "original list is not mutated".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.list.push".to_string()),
        module_path: "std::collections".to_string(),
        name: "push".to_string(),
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
                "first arg is List<T>".to_string(),
                "second arg is T".to_string(),
            ],
            ensures: vec![
                "result length equals input length plus one".to_string(),
                "new element is appended at the end".to_string(),
                "original list is not mutated".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.list.get".to_string()),
        module_path: "std::collections".to_string(),
        name: "get".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["T".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is List<T>".to_string(),
                "second arg is Int (index)".to_string(),
            ],
            ensures: vec![
                "Some(element) when 0 <= index < length".to_string(),
                "None when index >= length".to_string(),
                "None when index < 0".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.list.map".to_string()),
        module_path: "std::collections".to_string(),
        name: "map".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "List".to_string(),
            generics: vec!["T".to_string(), "U".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is List<T>".to_string(),
                "second arg is Fn(T) -> U".to_string(),
            ],
            ensures: vec![
                "result length equals input length".to_string(),
                "each result element is f applied to the corresponding input element".to_string(),
                "order is preserved".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.list.filter".to_string()),
        module_path: "std::collections".to_string(),
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
                "first arg is List<T>".to_string(),
                "second arg is Fn(T) -> Bool".to_string(),
            ],
            ensures: vec![
                "result contains only elements where predicate returns true".to_string(),
                "relative order of retained elements is preserved".to_string(),
                "result length <= input length".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.list.fold".to_string()),
        module_path: "std::collections".to_string(),
        name: "fold".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "U".to_string(),
            generics: vec!["T".to_string(), "U".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is List<T>".to_string(),
                "second arg is initial accumulator U".to_string(),
                "third arg is Fn(List([acc, item])) -> U (binary encoding: function receives List([acc, item]))".to_string(),
            ],
            ensures: vec![
                "empty list returns the initial accumulator unchanged".to_string(),
                "fold function is applied left-to-right".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.list.concat".to_string()),
        module_path: "std::collections".to_string(),
        name: "concat".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "List".to_string(),
            generics: vec!["T".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["both args are List<T>".to_string()],
            ensures: vec![
                "result contains all elements of the first list followed by the second".to_string(),
                "result length equals sum of both input lengths".to_string(),
                "neither input list is mutated".to_string(),
            ],
        }),
    });

    // ── std.collections map functions ─────────────────────────────────────

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.map.get".to_string()),
        module_path: "std::collections".to_string(),
        name: "get".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Option".to_string(),
            generics: vec!["V".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is Map<Text, V>".to_string(),
                "second arg is Text (key)".to_string(),
            ],
            ensures: vec![
                "Some(value) when key exists in the map".to_string(),
                "None when key is absent".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.map.contains_key".to_string()),
        module_path: "std::collections".to_string(),
        name: "contains_key".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Bool".to_string(),
            generics: vec!["Text".to_string(), "V".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is Map<Text, V>".to_string(),
                "second arg is Text (key)".to_string(),
            ],
            ensures: vec![
                "true when key exists in the map".to_string(),
                "false when key is absent".to_string(),
                "stored values are not exposed by the predicate".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.map.length".to_string()),
        module_path: "std::collections".to_string(),
        name: "length".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "UInt".to_string(),
            generics: vec!["Text".to_string(), "V".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["first arg is Map<Text, V>".to_string()],
            ensures: vec![
                "result >= 0".to_string(),
                "result equals the number of unique keys in the map".to_string(),
                "stored keys and values are not exposed by the count".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.map.insert".to_string()),
        module_path: "std::collections".to_string(),
        name: "insert".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Map".to_string(),
            generics: vec!["Text".to_string(), "V".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is Map<Text, V>".to_string(),
                "second arg is Text (key)".to_string(),
                "third arg is V (value)".to_string(),
            ],
            ensures: vec![
                "result contains the new key-value pair".to_string(),
                "any existing entry at key is replaced".to_string(),
                "original map is not mutated".to_string(),
            ],
        }),
    });

    // ── std.collections set functions ─────────────────────────────────────

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.set.contains".to_string()),
        module_path: "std::collections".to_string(),
        name: "contains".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Bool".to_string(),
            generics: vec!["T".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "first arg is List<T> (set representation)".to_string(),
                "second arg is T (element to test)".to_string(),
            ],
            ensures: vec![
                "true when element is equal to at least one entry".to_string(),
                "false when no entry matches".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.set.length".to_string()),
        module_path: "std::collections".to_string(),
        name: "length".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "UInt".to_string(),
            generics: vec!["T".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["first arg is List<T> (set representation)".to_string()],
            ensures: vec![
                "result >= 0".to_string(),
                "result equals the number of entries in the set representation".to_string(),
                "original set is not mutated".to_string(),
            ],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.collections.set.insert".to_string()),
        module_path: "std::collections".to_string(),
        name: "insert".to_string(),
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
                "first arg is List<T> (set representation)".to_string(),
                "second arg is T (element to insert)".to_string(),
            ],
            ensures: vec![
                "result contains the element".to_string(),
                "no duplicate entries are introduced".to_string(),
                "original set is not mutated".to_string(),
            ],
        }),
    });
}
