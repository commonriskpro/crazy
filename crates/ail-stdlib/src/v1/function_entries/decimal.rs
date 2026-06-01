use super::*;

pub(super) fn add_entries(reg: &mut StdlibRegistry) {
    reg.entries.push(decimal_entry(
        "std.decimal.from_int",
        "from_int",
        "Decimal",
        &[],
        &["input is Int"],
        &["returns Decimal with scale 0"],
    ));

    reg.entries.push(decimal_entry(
        "std.decimal.rescale",
        "rescale",
        "Result",
        &["Decimal", "Text"],
        &["input is Decimal plus target scale Int"],
        &[
            "Ok(Decimal) when rescale succeeds",
            "Err(Text) on overflow or invalid scale",
        ],
    ));

    reg.entries.push(decimal_entry(
        "std.decimal.add",
        "add",
        "Result",
        &["Decimal", "Text"],
        &["inputs are Decimals with matching scale"],
        &[
            "Ok(Decimal) when addition succeeds",
            "Err(Text) on scale mismatch or overflow",
        ],
    ));

    reg.entries.push(decimal_entry(
        "std.decimal.sub",
        "sub",
        "Result",
        &["Decimal", "Text"],
        &["inputs are Decimals with matching scale"],
        &[
            "Ok(Decimal) when subtraction succeeds",
            "Err(Text) on scale mismatch or overflow",
        ],
    ));

    reg.entries.push(decimal_entry(
        "std.decimal.mul",
        "mul",
        "Result",
        &["Decimal", "Text"],
        &["inputs are Decimals"],
        &[
            "Ok(Decimal) when multiplication succeeds",
            "Err(Text) on overflow",
        ],
    ));
}

fn decimal_entry(
    id: &str,
    name: &str,
    nominal: &str,
    generics: &[&str],
    requires: &[&str],
    ensures: &[&str],
) -> StdlibEntry {
    StdlibEntry {
        id: StdlibId(id.to_string()),
        module_path: "std::decimal".to_string(),
        name: name.to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: nominal.to_string(),
            generics: generics.iter().map(|generic| generic.to_string()).collect(),
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: requires.iter().map(|clause| clause.to_string()).collect(),
            ensures: ensures.iter().map(|clause| clause.to_string()).collect(),
        }),
    }
}
