use ail_stdlib::testing::{
    FixedClock, Golden, PropertyTest, SeededRandom, Test, TestResult, assert_approx,
    assert_eq as ail_assert_eq, expect_error, generate_cases_from_contract,
};

#[test]
fn assert_eq_pass() {
    let r = ail_assert_eq(&42i32, &42i32, "test");
    assert_eq!(r, TestResult::Pass);
}

#[test]
fn assert_eq_fail() {
    let r = ail_assert_eq(&1i32, &2i32, "mytest");
    assert!(r.is_fail());
    if let TestResult::Fail { message } = r {
        assert!(message.contains("mytest"));
        assert!(message.contains("1"));
        assert!(message.contains("2"));
    }
}

#[test]
fn assert_approx_pass() {
    let r = assert_approx(1.0, 1.0 + 1e-10, 1e-9, "approx");
    assert_eq!(r, TestResult::Pass);
}

#[test]
fn assert_approx_fail() {
    let r = assert_approx(1.0, 2.0, 0.5, "approx");
    assert!(r.is_fail());
}

#[test]
fn expect_error_on_err() {
    let result: Result<i32, &str> = Err("fail");
    let r = expect_error(&result, "must be err");
    assert_eq!(r, TestResult::Pass);
}

#[test]
fn expect_error_on_ok() {
    let result: Result<i32, &str> = Ok(42);
    let r = expect_error(&result, "must be err");
    assert!(r.is_fail());
}

#[test]
fn generate_cases_from_contract_all_pass() {
    let cases = vec![
        ("double 1", 1i32, 2i32),
        ("double 2", 2i32, 4i32),
        ("double 3", 3i32, 6i32),
    ];
    let results = generate_cases_from_contract(&cases, |x| x * 2);
    assert!(results.iter().all(|r| r.is_pass()));
}

#[test]
fn generate_cases_from_contract_some_fail() {
    let cases = vec![
        ("double 1", 1i32, 2i32),
        ("wrong", 2i32, 99i32), // intentionally wrong
    ];
    let results = generate_cases_from_contract(&cases, |x| x * 2);
    assert!(results[0].is_pass());
    assert!(results[1].is_fail());
}

#[test]
fn test_result_variants() {
    assert!(TestResult::Pass.is_pass());
    assert!(!TestResult::Pass.is_fail());
    let fail = TestResult::Fail {
        message: "x".into(),
    };
    assert!(fail.is_fail());
    assert!(!fail.is_pass());
}

#[test]
fn test_struct() {
    let t = Test::new("my_test").with_description("describes something");
    assert_eq!(t.name, "my_test");
    assert_eq!(t.description, Some("describes something".into()));
}

#[test]
fn property_test_struct() {
    let pt = PropertyTest::new("prop_test", "no overflow");
    assert_eq!(pt.contract, "no overflow");
}

#[test]
fn golden_pass() {
    let g = Golden::new("snapshot", "expected output");
    assert_eq!(g.check("expected output"), TestResult::Pass);
}

#[test]
fn golden_fail() {
    let g = Golden::new("snapshot", "expected");
    let r = g.check("actual");
    assert!(r.is_fail());
}

#[test]
fn fixed_clock() {
    let clock = FixedClock::new(1_700_000_000);
    assert_eq!(clock.unix_secs, 1_700_000_000);
}

#[test]
fn seeded_random() {
    let rng = SeededRandom::new(42);
    assert_eq!(rng.seed, 42);
}
