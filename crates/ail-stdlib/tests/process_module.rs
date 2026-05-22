use ail_stdlib::process::{ExitCode, ProcessError, ProcessHandle, ProcessId, Signal};

#[test]
fn exit_code_success() {
    let e = ExitCode::success();
    assert_eq!(e.0, 0);
    assert!(e.is_success());
}

#[test]
fn exit_code_failure() {
    let e = ExitCode::failure();
    assert!(!e.is_success());
}

#[test]
fn exit_code_display() {
    assert_eq!(format!("{}", ExitCode::success()), "0");
}

#[test]
fn process_handle_fields() {
    let h = ProcessHandle::new(ProcessId(42), "my-command");
    assert_eq!(h.id.0, 42);
    assert_eq!(h.command, "my-command");
}

#[test]
fn process_error_display() {
    assert!(format!("{}", ProcessError::PermissionDenied).contains("permission"));
    assert!(format!("{}", ProcessError::NotFound("cmd".into())).contains("not found"));
    assert!(format!("{}", ProcessError::SpawnFailed("err".into())).contains("spawn"));
    assert!(format!("{}", ProcessError::SignalFailed("err".into())).contains("signal"));
}

#[test]
fn signal_variants() {
    let _ = Signal::Terminate;
    let _ = Signal::Kill;
    let _ = Signal::Interrupt;
    let _ = Signal::Hangup;
    let _ = Signal::User1;
    let _ = Signal::User2;
}
