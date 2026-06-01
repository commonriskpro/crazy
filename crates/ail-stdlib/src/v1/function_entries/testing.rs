use super::*;

pub(super) fn add_entries(reg: &mut StdlibRegistry) {
    reg.entries.push(StdlibEntry {
        id: StdlibId("std.testing.assert_approx".to_string()),
        module_path: "std::testing".to_string(),
        name: "assert_approx".to_string(),
        kind: NodeKind::Function,
        stability: StabilityTier::Stable,
        type_facts: Some(TypeFacts {
            nominal: "Result".to_string(),
            generics: vec!["Unit".to_string(), "Text".to_string()],
        }),
        effect_row: None,
        capability_reqs: None,
        contract_clauses: Some(ContractClauses {
            requires: vec![
                "inputs are Float left/right/epsilon and Text context".to_string(),
                "epsilon comparison semantics chosen explicitly".to_string(),
            ],
            ensures: vec![
                "Ok(Unit) when |left - right| <= epsilon".to_string(),
                "Err(message) when values differ by more than epsilon".to_string(),
            ],
        }),
    });
}
