use ail_stdlib::boundary::{
    AdapterContract, Assumption, BoundaryDef, ForeignFunction, ForeignType, TrustLevel,
};

#[test]
fn trust_level_ordering() {
    assert!(TrustLevel::Untrusted < TrustLevel::External);
    assert!(TrustLevel::External < TrustLevel::Verified);
    assert!(TrustLevel::Verified < TrustLevel::Trusted);
}

#[test]
fn trust_level_display() {
    assert_eq!(format!("{}", TrustLevel::Trusted), "trusted");
    assert_eq!(format!("{}", TrustLevel::Untrusted), "untrusted");
}

#[test]
fn assumption_new() {
    let a = Assumption::new("A001", "Caller validates input", TrustLevel::Verified);
    assert_eq!(a.id, "A001");
    assert_eq!(a.trust_level, TrustLevel::Verified);
}

#[test]
fn foreign_type_new() {
    let ft = ForeignType::new("ExternalUser", "python_sdk", TrustLevel::External);
    assert_eq!(ft.name, "ExternalUser");
    assert_eq!(ft.origin, "python_sdk");
}

#[test]
fn foreign_function_with_assumption() {
    let assumption = Assumption::new("A1", "always succeeds", TrustLevel::External);
    let ff = ForeignFunction::new("calculate", "(i32) -> i32", TrustLevel::External)
        .with_assumption(assumption);
    assert_eq!(ff.name, "calculate");
    assert_eq!(ff.assumptions.len(), 1);
}

#[test]
fn adapter_contract_new() {
    let ac = AdapterContract::new("C001", "AilUser", "RawUser", TrustLevel::Verified);
    assert_eq!(ac.id, "C001");
    assert_eq!(ac.ail_type, "AilUser");
    assert_eq!(ac.foreign_type, "RawUser");
}

#[test]
fn boundary_def_new() {
    let bd = BoundaryDef::new("py-boundary", TrustLevel::External);
    assert_eq!(bd.id, "py-boundary");
    assert_eq!(bd.trust_level, TrustLevel::External);
    assert!(bd.foreign_types.is_empty());
    assert!(bd.foreign_functions.is_empty());
    assert!(bd.contracts.is_empty());
}

#[test]
fn boundary_def_default() {
    let bd = BoundaryDef::default();
    assert_eq!(bd.trust_level, TrustLevel::Untrusted);
}
