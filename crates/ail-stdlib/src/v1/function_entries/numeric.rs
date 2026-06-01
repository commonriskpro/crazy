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

    reg.entries.push(numeric_i64_entry(
        "std.numeric.wrapping_sub",
        "wrapping_sub",
        &["wrapping semantics chosen explicitly"],
        &["result wraps on underflow or overflow (defined, not silent)"],
    ));

    reg.entries.push(numeric_i64_entry(
        "std.numeric.wrapping_mul",
        "wrapping_mul",
        &["wrapping semantics chosen explicitly"],
        &["result wraps on overflow (defined, not silent)"],
    ));

    reg.entries.push(numeric_i64_entry(
        "std.numeric.wrapping_neg",
        "wrapping_neg",
        &["wrapping semantics chosen explicitly"],
        &["result wraps on negation overflow (defined, not silent)"],
    ));

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

    reg.entries.push(numeric_i64_entry(
        "std.numeric.saturating_sub",
        "saturating_sub",
        &["saturating semantics chosen explicitly"],
        &["result clamped to i64::MAX or i64::MIN on underflow or overflow"],
    ));

    reg.entries.push(numeric_i64_entry(
        "std.numeric.saturating_mul",
        "saturating_mul",
        &["saturating semantics chosen explicitly"],
        &["result clamped to i64::MAX or i64::MIN on overflow"],
    ));

    reg.entries.push(numeric_i64_entry(
        "std.numeric.saturating_neg",
        "saturating_neg",
        &["saturating semantics chosen explicitly"],
        &["i64::MIN negation clamps to i64::MAX"],
    ));

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

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.numeric.min".to_string()),
        module_path: "std::numeric".to_string(),
        name: "min".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "i64".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["signed integer comparison".to_string()],
            ensures: vec!["returns the smaller operand".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.numeric.max".to_string()),
        module_path: "std::numeric".to_string(),
        name: "max".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "i64".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["signed integer comparison".to_string()],
            ensures: vec!["returns the larger operand".to_string()],
        }),
    });

    reg.entries.push(StdlibEntry {
        id: StdlibId("std.numeric.clamp".to_string()),
        module_path: "std::numeric".to_string(),
        name: "clamp".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "i64".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec!["signed integer bounds".to_string()],
            ensures: vec![
                "returns low when value is below low".to_string(),
                "returns high when value is above high".to_string(),
                "returns value when value is inside bounds".to_string(),
            ],
        }),
    });

    reg.entries.push(numeric_i64_entry(
        "std.numeric.abs_or",
        "abs_or",
        &["fallback semantics chosen explicitly"],
        &["returns fallback when absolute value would overflow"],
    ));

    reg.entries.push(numeric_i64_entry(
        "std.numeric.neg_or",
        "neg_or",
        &["fallback semantics chosen explicitly"],
        &["returns fallback when negation would overflow"],
    ));

    reg.entries.push(numeric_i64_entry(
        "std.numeric.add_or",
        "add_or",
        &["fallback semantics chosen explicitly"],
        &["returns fallback on addition overflow"],
    ));

    reg.entries.push(numeric_i64_entry(
        "std.numeric.sub_or",
        "sub_or",
        &["fallback semantics chosen explicitly"],
        &["returns fallback on subtraction underflow or overflow"],
    ));

    reg.entries.push(numeric_i64_entry(
        "std.numeric.mul_or",
        "mul_or",
        &["fallback semantics chosen explicitly"],
        &["returns fallback on multiplication overflow"],
    ));

    reg.entries.push(numeric_i64_entry(
        "std.numeric.div_or",
        "div_or",
        &["fallback semantics chosen explicitly"],
        &["returns fallback on divide-by-zero or division overflow"],
    ));

    reg.entries.push(numeric_i64_entry(
        "std.numeric.rem_or",
        "rem_or",
        &["fallback semantics chosen explicitly"],
        &["returns fallback on divide-by-zero or remainder overflow"],
    ));

    reg.entries.push(numeric_i64_entry(
        "std.numeric.bit_and",
        "bit_and",
        &["bitwise integer semantics chosen explicitly"],
        &["returns left AND right"],
    ));

    reg.entries.push(numeric_i64_entry(
        "std.numeric.bit_or",
        "bit_or",
        &["bitwise integer semantics chosen explicitly"],
        &["returns left OR right"],
    ));

    reg.entries.push(numeric_i64_entry(
        "std.numeric.bit_xor",
        "bit_xor",
        &["bitwise integer semantics chosen explicitly"],
        &["returns left XOR right"],
    ));

    reg.entries.push(numeric_i64_entry(
        "std.numeric.bit_not",
        "bit_not",
        &["bitwise integer semantics chosen explicitly"],
        &["returns bitwise complement"],
    ));

    reg.entries.push(numeric_i64_entry(
        "std.numeric.shift_left",
        "shift_left",
        &["wrapping shift semantics chosen explicitly"],
        &["returns value shifted left by amount modulo machine shift width"],
    ));

    reg.entries.push(numeric_i64_entry(
        "std.numeric.shift_right",
        "shift_right",
        &["wrapping signed shift semantics chosen explicitly"],
        &["returns arithmetic right shift by amount modulo machine shift width"],
    ));

    reg.entries.push(numeric_i64_entry(
        "std.numeric.shift_right_unsigned",
        "shift_right_unsigned",
        &["wrapping unsigned shift semantics chosen explicitly"],
        &["returns logical right shift by amount modulo machine shift width"],
    ));
}

fn numeric_i64_entry(id: &str, name: &str, requires: &[&str], ensures: &[&str]) -> StdlibEntry {
    StdlibEntry {
        id: StdlibId(id.to_string()),
        module_path: "std::numeric".to_string(),
        name: name.to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "i64".to_string(),
            generics: vec![],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: requires.iter().map(|clause| clause.to_string()).collect(),
            ensures: ensures.iter().map(|clause| clause.to_string()).collect(),
        }),
    }
}
