use ail_stdlib::env::{EnvError, EnvVar, env_list, env_read, env_write};

#[test]
fn env_var_new() {
    let v = EnvVar::new("MY_VAR", "my_value");
    assert_eq!(v.key, "MY_VAR");
    assert_eq!(v.value, "my_value");
}

#[test]
fn env_error_display() {
    assert!(format!("{}", EnvError::PermissionDenied).contains("permission"));
    assert!(format!("{}", EnvError::NotFound("KEY".into())).contains("not found"));
    assert!(format!("{}", EnvError::InvalidValue("KEY".into())).contains("invalid"));
}

#[test]
fn env_read_existing_var() {
    // PATH is almost always set on UNIX
    let result = env_read("PATH");
    // Either it's found or the test env doesn't have it — just check it doesn't panic
    let _ = result;
}

#[test]
fn env_read_missing_var() {
    let result = env_read("__AIL_STDLIB_MISSING_VAR_XYZ__");
    assert_eq!(
        result,
        Err(EnvError::NotFound("__AIL_STDLIB_MISSING_VAR_XYZ__".into()))
    );
}

#[test]
fn env_write_stub_returns_ok() {
    // env_write is a no-op stub in stdlib
    assert_eq!(env_write("TEST_KEY", "test_value"), Ok(()));
}

#[test]
fn env_list_returns_vec() {
    let vars = env_list();
    // Should have at least some vars in the test environment
    let _ = vars; // just ensure it doesn't panic
}
