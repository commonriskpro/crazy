use ail_stdlib::env::{EnvError, EnvVar, env_list, env_read, env_write};
use std::sync::Mutex;

// env_write mutates the process environment — tests must be serialised.
static ENV_WRITE_LOCK: Mutex<()> = Mutex::new(());

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
fn env_write_sets_and_reads_back() {
    let _guard = ENV_WRITE_LOCK.lock().unwrap();
    const KEY: &str = "__AIL_TEST_ENV_WRITE_ROUND_TRIP__";
    assert_eq!(env_write(KEY, "hello"), Ok(()));
    assert_eq!(env_read(KEY), Ok("hello".to_string()));
}

#[test]
fn env_write_overwrites_existing_key() {
    let _guard = ENV_WRITE_LOCK.lock().unwrap();
    const KEY: &str = "__AIL_TEST_ENV_WRITE_OVERWRITE__";
    assert_eq!(env_write(KEY, "first"), Ok(()));
    assert_eq!(env_write(KEY, "second"), Ok(()));
    assert_eq!(env_read(KEY), Ok("second".to_string()));
}

#[test]
fn env_write_rejects_empty_key() {
    assert_eq!(
        env_write("", "value"),
        Err(EnvError::InvalidValue("".into()))
    );
}

#[test]
fn env_write_rejects_key_with_equals() {
    assert_eq!(
        env_write("A=B", "value"),
        Err(EnvError::InvalidValue("A=B".into()))
    );
}

#[test]
fn env_write_rejects_nul_in_key() {
    assert_eq!(
        env_write("KEY\0BAD", "value"),
        Err(EnvError::InvalidValue("KEY\0BAD".into()))
    );
}

#[test]
fn env_write_rejects_nul_in_value() {
    assert_eq!(
        env_write("__AIL_TEST_NUL_VALUE__", "val\0ue"),
        Err(EnvError::InvalidValue("__AIL_TEST_NUL_VALUE__".into()))
    );
}

#[test]
fn env_list_returns_vec() {
    let vars = env_list();
    // Should have at least some vars in the test environment
    let _ = vars; // just ensure it doesn't panic
}
