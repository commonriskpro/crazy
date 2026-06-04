// Stable machine-readable metadata for dogfood program/fixture expectations.
// This stays local to `ail-dogfood`: it describes examples and validates the
// metadata contract before any compiler/runtime fixture runner consumes it.

pub const FIXTURE_CONTRACT_SCHEMA_VERSION: &str = "ail-dogfood.fixture-contract/1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixtureExpectedOutcome {
    Pass,
    Fail,
}

impl FixtureExpectedOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpectedFixtureDiagnostic<'a> {
    pub code: &'a str,
    pub target: &'a str,
    pub blocking: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixtureContract<'a> {
    pub name: &'a str,
    pub capabilities: &'a [&'a str],
    pub expected_outcome: FixtureExpectedOutcome,
    pub expected_diagnostics: &'a [ExpectedFixtureDiagnostic<'a>],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixtureContractIssueCode {
    ContractNameMissing,
    ContractNameInvalid,
    ContractNameDuplicate,
    CapabilityInvalid,
    CapabilityDuplicate,
    ExpectedDiagnosticCodeMissing,
    ExpectedDiagnosticTargetMissing,
    ExpectedDiagnosticDuplicate,
    PassingFixtureHasDiagnostics,
}

impl FixtureContractIssueCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ContractNameMissing => "dogfood.fixture.contract_name.missing",
            Self::ContractNameInvalid => "dogfood.fixture.contract_name.invalid",
            Self::ContractNameDuplicate => "dogfood.fixture.contract_name.duplicate",
            Self::CapabilityInvalid => "dogfood.fixture.capability.invalid",
            Self::CapabilityDuplicate => "dogfood.fixture.capability.duplicate",
            Self::ExpectedDiagnosticCodeMissing => {
                "dogfood.fixture.expected_diagnostic.code_missing"
            }
            Self::ExpectedDiagnosticTargetMissing => {
                "dogfood.fixture.expected_diagnostic.target_missing"
            }
            Self::ExpectedDiagnosticDuplicate => "dogfood.fixture.expected_diagnostic.duplicate",
            Self::PassingFixtureHasDiagnostics => "dogfood.fixture.outcome.pass_has_diagnostics",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureContractIssue {
    pub contract_name: String,
    pub code: FixtureContractIssueCode,
    pub field: &'static str,
    pub value: String,
}

impl FixtureContractIssue {
    fn new(
        contract_name: impl Into<String>,
        code: FixtureContractIssueCode,
        field: &'static str,
        value: impl Into<String>,
    ) -> Self {
        Self {
            contract_name: contract_name.into(),
            code,
            field,
            value: value.into(),
        }
    }
}

const NO_CAPABILITIES: &[&str] = &[];
const NO_DIAGNOSTICS: &[ExpectedFixtureDiagnostic<'static>] = &[];
const LOGGER_CAPABILITIES: &[&str] = &["cap.logger"];

const DOGFOOD_PROGRAM_CONTRACTS: &[FixtureContract<'static>] = &[
    FixtureContract {
        name: "dogfood.program.calculator_sum_of_squares",
        capabilities: NO_CAPABILITIES,
        expected_outcome: FixtureExpectedOutcome::Pass,
        expected_diagnostics: NO_DIAGNOSTICS,
    },
    FixtureContract {
        name: "dogfood.program.conditionals_abs_max_clamp",
        capabilities: NO_CAPABILITIES,
        expected_outcome: FixtureExpectedOutcome::Pass,
        expected_diagnostics: NO_DIAGNOSTICS,
    },
    FixtureContract {
        name: "dogfood.program.logger_effect",
        capabilities: LOGGER_CAPABILITIES,
        expected_outcome: FixtureExpectedOutcome::Pass,
        expected_diagnostics: NO_DIAGNOSTICS,
    },
];

pub fn dogfood_program_contracts() -> &'static [FixtureContract<'static>] {
    DOGFOOD_PROGRAM_CONTRACTS
}

pub fn fixture_contracts_in_validation_order<'contracts, 'data>(
    contracts: &'contracts [FixtureContract<'data>],
) -> Vec<&'contracts FixtureContract<'data>> {
    let mut ordered: Vec<&FixtureContract<'data>> = contracts.iter().collect();
    ordered.sort_by(|left, right| {
        contract_name(left)
            .cmp(contract_name(right))
            .then_with(|| left.capabilities.len().cmp(&right.capabilities.len()))
            .then_with(|| {
                left.expected_diagnostics
                    .len()
                    .cmp(&right.expected_diagnostics.len())
            })
    });
    ordered
}

pub fn validate_fixture_contracts(contracts: &[FixtureContract<'_>]) -> Vec<FixtureContractIssue> {
    let mut issues = Vec::new();
    let mut seen_names = std::collections::BTreeSet::new();

    for contract in fixture_contracts_in_validation_order(contracts) {
        let name = contract_name(contract).to_string();
        validate_name(contract.name.trim(), &name, &mut seen_names, &mut issues);
        validate_capabilities(contract.capabilities, &name, &mut issues);
        validate_expected_diagnostics(contract, &name, &mut issues);
    }

    issues.sort_by(|left, right| {
        (
            left.contract_name.as_str(),
            left.code.as_str(),
            left.field,
            left.value.as_str(),
        )
            .cmp(&(
                right.contract_name.as_str(),
                right.code.as_str(),
                right.field,
                right.value.as_str(),
            ))
    });
    issues
}

fn validate_name<'a>(
    raw: &'a str,
    name: &str,
    seen_names: &mut std::collections::BTreeSet<&'a str>,
    issues: &mut Vec<FixtureContractIssue>,
) {
    if raw.is_empty() {
        issues.push(issue(
            name,
            FixtureContractIssueCode::ContractNameMissing,
            "name",
            "<empty>",
        ));
    } else {
        if !is_machine_name(raw) {
            issues.push(issue(
                name,
                FixtureContractIssueCode::ContractNameInvalid,
                "name",
                raw,
            ));
        }
        if !seen_names.insert(raw) {
            issues.push(issue(
                name,
                FixtureContractIssueCode::ContractNameDuplicate,
                "name",
                raw,
            ));
        }
    }
}

fn validate_capabilities(
    capabilities: &[&str],
    name: &str,
    issues: &mut Vec<FixtureContractIssue>,
) {
    let mut seen = std::collections::BTreeSet::new();
    for capability in capabilities.iter().map(|capability| capability.trim()) {
        if !is_capability_name(capability) {
            issues.push(issue(
                name,
                FixtureContractIssueCode::CapabilityInvalid,
                "capabilities",
                redacted(capability),
            ));
        }
        if !capability.is_empty() && !seen.insert(capability) {
            issues.push(issue(
                name,
                FixtureContractIssueCode::CapabilityDuplicate,
                "capabilities",
                redacted(capability),
            ));
        }
    }
}

fn validate_expected_diagnostics(
    contract: &FixtureContract<'_>,
    name: &str,
    issues: &mut Vec<FixtureContractIssue>,
) {
    if contract.expected_outcome == FixtureExpectedOutcome::Pass
        && !contract.expected_diagnostics.is_empty()
    {
        issues.push(issue(
            name,
            FixtureContractIssueCode::PassingFixtureHasDiagnostics,
            "expected_diagnostics",
            contract.expected_diagnostics.len().to_string(),
        ));
    }

    let mut seen = std::collections::BTreeSet::new();
    for diagnostic in contract.expected_diagnostics {
        let code = diagnostic.code.trim();
        let target = diagnostic.target.trim();
        if code.is_empty() {
            issues.push(issue(
                name,
                FixtureContractIssueCode::ExpectedDiagnosticCodeMissing,
                "expected_diagnostics.code",
                "<empty>",
            ));
        }
        if target.is_empty() {
            issues.push(issue(
                name,
                FixtureContractIssueCode::ExpectedDiagnosticTargetMissing,
                "expected_diagnostics.target",
                "<empty>",
            ));
        }
        if !code.is_empty() && !target.is_empty() && !seen.insert((code, target)) {
            issues.push(issue(
                name,
                FixtureContractIssueCode::ExpectedDiagnosticDuplicate,
                "expected_diagnostics",
                format!("{code}@{target}"),
            ));
        }
    }
}

fn issue(
    name: &str,
    code: FixtureContractIssueCode,
    field: &'static str,
    value: impl Into<String>,
) -> FixtureContractIssue {
    FixtureContractIssue::new(name, code, field, value)
}

fn contract_name<'a>(contract: &FixtureContract<'a>) -> &'a str {
    let name = contract.name.trim();
    if name.is_empty() { "<unnamed>" } else { name }
}

fn is_capability_name(value: &str) -> bool {
    value
        .strip_prefix("cap.")
        .is_some_and(|rest| !rest.is_empty() && is_machine_token(rest))
}

fn is_machine_name(value: &str) -> bool {
    !value.is_empty() && value.contains('.') && is_machine_token(value)
}

fn is_machine_token(value: &str) -> bool {
    value.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
    })
}

fn redacted(value: &str) -> String {
    if value.is_empty() {
        "<empty>".to_string()
    } else {
        value.to_string()
    }
}
