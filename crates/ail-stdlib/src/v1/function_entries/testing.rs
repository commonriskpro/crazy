use super::*;

pub(super) fn add_entries(reg: &mut StdlibRegistry) {
    reg.entries.push(testing_result_entry(
        "std.testing.assert_approx",
        "assert_approx",
        &[
            "inputs are Float left/right/epsilon and Text context",
            "epsilon comparison semantics chosen explicitly",
        ],
        &[
            "Ok(Unit) when |left - right| <= epsilon",
            "Err(message) when values differ by more than epsilon",
        ],
    ));

    reg.entries.push(testing_result_entry(
        "std.testing.assert_eq",
        "assert_eq",
        &["inputs are comparable values and Text context"],
        &[
            "Ok(Unit) when values are equal",
            "Err(message) when values are different",
        ],
    ));

    reg.entries.push(testing_result_entry(
        "std.testing.expect_error",
        "expect_error",
        &["input is Result<T, E> plus Text context"],
        &[
            "Ok(Unit) when input is Err",
            "Err(message) when input is Ok",
        ],
    ));
}

fn testing_result_entry(id: &str, name: &str, requires: &[&str], ensures: &[&str]) -> StdlibEntry {
    StdlibEntry {
        id: StdlibId(id.to_string()),
        module_path: "std::testing".to_string(),
        name: name.to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["Unit".to_string(), "Text".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: requires.iter().map(|clause| clause.to_string()).collect(),
            ensures: ensures.iter().map(|clause| clause.to_string()).collect(),
        }),
    }
}
