use ail_dogfood::fixture_contracts::{
    ExpectedFixtureDiagnostic, FIXTURE_CONTRACT_SCHEMA_VERSION, FixtureContract,
    FixtureContractIssue, FixtureContractIssueCode, FixtureExpectedOutcome,
    dogfood_program_contracts, fixture_contracts_in_validation_order, validate_fixture_contracts,
};

type Key = (String, String, &'static str, String);

fn keys(issues: &[FixtureContractIssue]) -> Vec<Key> {
    issues
        .iter()
        .map(|issue| {
            (
                issue.contract_name.clone(),
                issue.code.as_str().to_string(),
                issue.field,
                issue.value.clone(),
            )
        })
        .collect()
}

fn key(name: &str, code: FixtureContractIssueCode, field: &'static str, value: &str) -> Key {
    (name.into(), code.as_str().into(), field, value.into())
}

#[test]
fn dogfood_program_contract_catalog_is_valid_and_machine_readable() {
    assert_eq!(
        FIXTURE_CONTRACT_SCHEMA_VERSION,
        "ail-dogfood.fixture-contract/1"
    );

    let contracts = dogfood_program_contracts();

    assert_eq!(validate_fixture_contracts(contracts), Vec::new());
    assert_eq!(
        contracts
            .iter()
            .map(|contract| (
                contract.name,
                contract.capabilities,
                contract.expected_outcome.as_str(),
            ))
            .collect::<Vec<_>>(),
        vec![
            ("dogfood.program.calculator_sum_of_squares", &[][..], "pass",),
            (
                "dogfood.program.conditionals_abs_max_clamp",
                &[][..],
                "pass",
            ),
            ("dogfood.program.logger_effect", &["cap.logger"][..], "pass",),
        ]
    );
}

#[test]
fn validation_reports_machine_readable_issues_in_deterministic_order() {
    let diagnostics = [
        ExpectedFixtureDiagnostic {
            code: "",
            target: "",
            blocking: true,
        },
        ExpectedFixtureDiagnostic {
            code: "E_CONTRACT_VIOLATED",
            target: "fn.checkout",
            blocking: true,
        },
        ExpectedFixtureDiagnostic {
            code: "E_CONTRACT_VIOLATED",
            target: "fn.checkout",
            blocking: true,
        },
    ];
    let contracts = [
        FixtureContract {
            name: "dogfood.bad.zeta",
            capabilities: &["logger", "cap.logger", "cap.logger"],
            expected_outcome: FixtureExpectedOutcome::Pass,
            expected_diagnostics: &diagnostics,
        },
        FixtureContract {
            name: "dogfood.bad.alpha",
            capabilities: &[],
            expected_outcome: FixtureExpectedOutcome::Fail,
            expected_diagnostics: &[],
        },
        FixtureContract {
            name: "dogfood.bad.alpha",
            capabilities: &[],
            expected_outcome: FixtureExpectedOutcome::Pass,
            expected_diagnostics: &[],
        },
        FixtureContract {
            name: "Dogfood Bad Name",
            capabilities: &[],
            expected_outcome: FixtureExpectedOutcome::Pass,
            expected_diagnostics: &[],
        },
    ];

    assert_eq!(
        keys(&validate_fixture_contracts(&contracts)),
        vec![
            key(
                "Dogfood Bad Name",
                FixtureContractIssueCode::ContractNameInvalid,
                "name",
                "Dogfood Bad Name",
            ),
            key(
                "dogfood.bad.alpha",
                FixtureContractIssueCode::ContractNameDuplicate,
                "name",
                "dogfood.bad.alpha",
            ),
            key(
                "dogfood.bad.zeta",
                FixtureContractIssueCode::CapabilityDuplicate,
                "capabilities",
                "cap.logger",
            ),
            key(
                "dogfood.bad.zeta",
                FixtureContractIssueCode::CapabilityInvalid,
                "capabilities",
                "logger",
            ),
            key(
                "dogfood.bad.zeta",
                FixtureContractIssueCode::ExpectedDiagnosticCodeMissing,
                "expected_diagnostics.code",
                "<empty>",
            ),
            key(
                "dogfood.bad.zeta",
                FixtureContractIssueCode::ExpectedDiagnosticDuplicate,
                "expected_diagnostics",
                "E_CONTRACT_VIOLATED@fn.checkout",
            ),
            key(
                "dogfood.bad.zeta",
                FixtureContractIssueCode::ExpectedDiagnosticTargetMissing,
                "expected_diagnostics.target",
                "<empty>",
            ),
            key(
                "dogfood.bad.zeta",
                FixtureContractIssueCode::PassingFixtureHasDiagnostics,
                "expected_diagnostics",
                "3",
            ),
        ]
    );
}

#[test]
fn validation_order_is_canonical_even_when_contracts_are_shuffled() {
    let diagnostics = [ExpectedFixtureDiagnostic {
        code: "E_CONTRACT_VIOLATED",
        target: "",
        blocking: true,
    }];
    let alpha = FixtureContract {
        name: "dogfood.bad.alpha",
        capabilities: &[],
        expected_outcome: FixtureExpectedOutcome::Fail,
        expected_diagnostics: &diagnostics,
    };
    let zeta = FixtureContract {
        name: "dogfood.bad.zeta",
        capabilities: &["logger"],
        expected_outcome: FixtureExpectedOutcome::Fail,
        expected_diagnostics: &[],
    };

    let original = [zeta, alpha];
    let shuffled = [alpha, zeta];

    assert_eq!(
        fixture_contracts_in_validation_order(&original)
            .iter()
            .map(|contract| contract.name)
            .collect::<Vec<_>>(),
        vec!["dogfood.bad.alpha", "dogfood.bad.zeta"]
    );
    assert_eq!(
        keys(&validate_fixture_contracts(&original)),
        keys(&validate_fixture_contracts(&shuffled))
    );
}
