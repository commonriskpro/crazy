use ail_stdlib::exec::{
    InMemoryCapabilityHost, StdlibCapabilityDispatch, StdlibExecError, StdlibValue,
    call_effectful_stdlib, call_pure_stdlib,
};

#[test]
fn path_from_text_returns_runtime_path_value() {
    let result = call_pure_stdlib(
        "std.path.from_text",
        &[StdlibValue::Text("/tmp/config.toml".to_string())],
    );

    assert_eq!(
        result,
        Ok(StdlibValue::Path("/tmp/config.toml".to_string()))
    );
}

#[test]
fn path_to_text_returns_original_text() {
    let result = call_pure_stdlib(
        "std.path.to_text",
        &[StdlibValue::Path("/tmp/config.toml".to_string())],
    );

    assert_eq!(
        result,
        Ok(StdlibValue::Text("/tmp/config.toml".to_string()))
    );
}

#[test]
fn path_to_text_rejects_plain_text() {
    let result = call_pure_stdlib(
        "std.path.to_text",
        &[StdlibValue::Text("/tmp/config.toml".to_string())],
    );

    assert_eq!(result, Err(StdlibExecError::Type { expected: "Path" }));
}

#[test]
fn fs_read_file_accepts_runtime_path_value() {
    let host = InMemoryCapabilityHost::new().with_file("/hello.txt", b"world");
    let result = call_effectful_stdlib(
        "std.fs.read_file",
        &[StdlibValue::Path("/hello.txt".to_string())],
        &host,
    );

    assert_eq!(result, Ok(StdlibValue::Bytes(b"world".to_vec())));
}

#[test]
fn in_memory_host_write_delete_and_list_use_path_values() {
    let host = InMemoryCapabilityHost::new()
        .with_file("/workspace/old.txt", b"old")
        .with_file("/workspace/stable.txt", b"stable");

    assert_eq!(
        host.call(
            "file.write",
            "write",
            &[
                StdlibValue::Path("/workspace/new.txt".to_string()),
                StdlibValue::Bytes(b"new".to_vec()),
            ],
        ),
        Ok(StdlibValue::Unit)
    );
    assert_eq!(
        host.call(
            "file.delete",
            "delete",
            &[StdlibValue::Path("/workspace/old.txt".to_string())],
        ),
        Ok(StdlibValue::Unit)
    );

    let listed = host.call(
        "file.list",
        "list",
        &[StdlibValue::Path("/workspace".to_string())],
    );

    assert_eq!(
        listed,
        Ok(StdlibValue::List(vec![
            StdlibValue::Path("/workspace/new.txt".to_string()),
            StdlibValue::Path("/workspace/stable.txt".to_string()),
        ]))
    );
}
