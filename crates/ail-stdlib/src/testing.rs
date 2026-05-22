// ── ail-stdlib::testing ───────────────────────────────────────────────────
//
// Testing helpers for the AIL `std.testing` module.
//
// # Rules (from docs/stdlib.md)
//
// - tests are evidence, not automatic proof
// - property tests link to contracts/invariants

// ── TestResult ────────────────────────────────────────────────────────────

/// The result of a single test or assertion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestResult {
    Pass,
    Fail { message: String },
    Skip { reason: String },
}

impl TestResult {
    pub fn is_pass(&self) -> bool {
        matches!(self, TestResult::Pass)
    }
    pub fn is_fail(&self) -> bool {
        matches!(self, TestResult::Fail { .. })
    }
}

// ── assert_eq ─────────────────────────────────────────────────────────────

/// Assert that two values are equal.
///
/// Returns `TestResult::Pass` on equality, `TestResult::Fail` with a
/// human-readable message on inequality.
pub fn assert_eq<T>(left: &T, right: &T, context: &str) -> TestResult
where
    T: PartialEq + std::fmt::Debug,
{
    if left == right {
        TestResult::Pass
    } else {
        TestResult::Fail {
            message: format!("{context}: expected {:?} == {:?}", left, right),
        }
    }
}

// ── assert_approx ─────────────────────────────────────────────────────────

/// Assert that two `f64` values are within `epsilon` of each other.
pub fn assert_approx(left: f64, right: f64, epsilon: f64, context: &str) -> TestResult {
    if (left - right).abs() <= epsilon {
        TestResult::Pass
    } else {
        TestResult::Fail {
            message: format!(
                "{context}: |{left} - {right}| = {} > epsilon {epsilon}",
                (left - right).abs()
            ),
        }
    }
}

// ── expect_error ──────────────────────────────────────────────────────────

/// Assert that a `Result` is an `Err`.
///
/// Returns `TestResult::Pass` if the result is `Err`; otherwise `Fail`.
pub fn expect_error<T, E: std::fmt::Debug>(result: &Result<T, E>, context: &str) -> TestResult {
    match result {
        Err(_) => TestResult::Pass,
        Ok(_) => TestResult::Fail {
            message: format!("{context}: expected Err, got Ok"),
        },
    }
}

// ── generate_cases_from_contract ──────────────────────────────────────────

/// Generate test cases from a contract specification.
///
/// A contract is a list of `(label, input, expected)` triples where
/// `check_fn` is applied to `input` and the result compared against `expected`.
///
/// Returns a `Vec<TestResult>` — one per case.
pub fn generate_cases_from_contract<Input, Output>(
    cases: &[(&str, Input, Output)],
    check_fn: impl Fn(&Input) -> Output,
) -> Vec<TestResult>
where
    Output: PartialEq + std::fmt::Debug,
    Input: std::fmt::Debug,
{
    cases
        .iter()
        .map(|(label, input, expected)| {
            let actual = check_fn(input);
            assert_eq(&actual, expected, label)
        })
        .collect()
}

// ── Test / PropertyTest / Fixture / Golden ────────────────────────────────

/// Metadata for a single test.
#[derive(Clone, Debug)]
pub struct Test {
    pub name: String,
    pub description: Option<String>,
}

impl Test {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
        }
    }
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

/// Metadata for a property-based test, linked to a contract/invariant.
#[derive(Clone, Debug)]
pub struct PropertyTest {
    pub name: String,
    pub contract: String,
}

impl PropertyTest {
    pub fn new(name: impl Into<String>, contract: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            contract: contract.into(),
        }
    }
}

/// A test fixture providing shared setup/teardown state.
pub trait Fixture {
    type State;
    fn setup() -> Self::State;
    fn teardown(state: Self::State);
}

/// A golden-file test assertion.
#[derive(Clone, Debug)]
pub struct Golden {
    pub name: String,
    pub expected: String,
}

impl Golden {
    pub fn new(name: impl Into<String>, expected: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            expected: expected.into(),
        }
    }

    pub fn check(&self, actual: &str) -> TestResult {
        if actual == self.expected {
            TestResult::Pass
        } else {
            TestResult::Fail {
                message: format!(
                    "golden '{}': expected:\n{}\ngot:\n{}",
                    self.name, self.expected, actual
                ),
            }
        }
    }
}

// ── Test handlers ─────────────────────────────────────────────────────────

/// A fixed-value clock for testing time-dependent code.
#[derive(Clone, Copy, Debug)]
pub struct FixedClock {
    pub unix_secs: i64,
}

impl FixedClock {
    pub fn new(unix_secs: i64) -> Self {
        Self { unix_secs }
    }
}

/// A seeded random generator for deterministic property tests.
pub struct SeededRandom {
    pub seed: u64,
}

impl SeededRandom {
    pub fn new(seed: u64) -> Self {
        Self { seed }
    }
}

/// A marker for in-memory database (implementation provided by host).
pub struct InMemoryDb;

/// A marker for recorded HTTP interactions.
pub struct RecordedHttp;

/// A marker for fake/mock handlers.
pub struct FakeHandler;
