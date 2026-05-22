use std::collections::BTreeMap;

use ail_stdlib::exec::{
    FunctionImpl, StdlibExecError, StdlibValue, call_pure_stdlib, find_function_entry,
    stdlib_function_entries,
};
use ail_stdlib::v1_registry_with_functions;

fn double(value: StdlibValue) -> Result<StdlibValue, StdlibExecError> {
    match value {
        StdlibValue::Int(value) => Ok(StdlibValue::Int(value * 2)),
        _ => Err(StdlibExecError::Type { expected: "Int" }),
    }
}

fn some_double(value: StdlibValue) -> Result<StdlibValue, StdlibExecError> {
    double(value).map(|value| StdlibValue::Option(Some(Box::new(value))))
}

#[test]
fn pure_function_entries_carry_rust_implementations() {
    let entry = find_function_entry("std.text.trim").expect("std.text.trim entry");

    assert_eq!(entry.module, "std.text");
    assert_eq!(entry.name, "trim");
    assert!(matches!(entry.implementation, FunctionImpl::Pure(_)));
    assert_eq!(
        entry.call(&[StdlibValue::Text("  ail  ".to_string())]),
        Ok(StdlibValue::Text("ail".to_string()))
    );
}

#[test]
fn capability_entries_are_dispatch_descriptors_not_pure_functions() {
    let entry = find_function_entry("std.fs.read").expect("std.fs.read entry");

    match entry.implementation {
        FunctionImpl::Capability {
            capability,
            operation,
        } => {
            assert_eq!(capability, "file.read");
            assert_eq!(operation, "read");
        }
        FunctionImpl::Pure(_) => panic!("std.fs.read must be capability-mediated"),
    }

    assert_eq!(
        entry.call(&[StdlibValue::Text("/config/app.toml".to_string())]),
        Err(StdlibExecError::CapabilityRequired {
            capability: "file.read".to_string(),
            operation: "read".to_string(),
        })
    );
}

#[test]
fn core_option_and_result_helpers_execute() {
    assert_eq!(
        call_pure_stdlib(
            "std.core.option.map",
            &[
                StdlibValue::Option(Some(Box::new(StdlibValue::Int(21)))),
                StdlibValue::Function(double),
            ],
        ),
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Int(42)))))
    );

    assert_eq!(
        call_pure_stdlib(
            "std.core.option.and_then",
            &[
                StdlibValue::Option(Some(Box::new(StdlibValue::Int(21)))),
                StdlibValue::Function(some_double),
            ],
        ),
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Int(42)))))
    );

    assert_eq!(
        call_pure_stdlib(
            "std.core.option.unwrap_or",
            &[
                StdlibValue::Option(None),
                StdlibValue::Text("fallback".into())
            ],
        ),
        Ok(StdlibValue::Text("fallback".into()))
    );

    assert_eq!(
        call_pure_stdlib(
            "std.core.option.ok_or",
            &[
                StdlibValue::Option(None),
                StdlibValue::Text("missing".into())
            ],
        ),
        Ok(StdlibValue::Result(Err(Box::new(StdlibValue::Text(
            "missing".into()
        )))))
    );

    assert_eq!(
        call_pure_stdlib(
            "std.core.result.map",
            &[
                StdlibValue::Result(Ok(Box::new(StdlibValue::Int(7)))),
                StdlibValue::Function(double),
            ],
        ),
        Ok(StdlibValue::Result(Ok(Box::new(StdlibValue::Int(14)))))
    );
}

#[test]
fn collections_execute_without_capabilities() {
    let list = StdlibValue::List(vec![StdlibValue::Int(1), StdlibValue::Int(2)]);

    assert_eq!(
        call_pure_stdlib("std.collections.list.length", std::slice::from_ref(&list)),
        Ok(StdlibValue::Int(2))
    );

    assert_eq!(
        call_pure_stdlib(
            "std.collections.list.get",
            &[list.clone(), StdlibValue::Int(1)],
        ),
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Int(2)))))
    );

    assert_eq!(
        call_pure_stdlib(
            "std.collections.set.insert",
            &[list.clone(), StdlibValue::Int(2)],
        ),
        Ok(list.clone())
    );

    let mut map = BTreeMap::new();
    map.insert("answer".to_string(), StdlibValue::Int(42));
    assert_eq!(
        call_pure_stdlib(
            "std.collections.map.get",
            &[StdlibValue::Map(map), StdlibValue::Text("answer".into())],
        ),
        Ok(StdlibValue::Option(Some(Box::new(StdlibValue::Int(42)))))
    );
}

#[test]
fn text_and_crypto_execute_without_capabilities() {
    assert_eq!(
        call_pure_stdlib(
            "std.text.format",
            &[
                StdlibValue::Text("hello {}, {}".into()),
                StdlibValue::List(vec![
                    StdlibValue::Text("std".into()),
                    StdlibValue::Text("AIL".into()),
                ]),
            ],
        ),
        Ok(StdlibValue::Text("hello std, AIL".into()))
    );

    assert_eq!(
        call_pure_stdlib("std.text.decode", &[StdlibValue::Bytes(vec![0xff])]),
        Ok(StdlibValue::Result(Err(Box::new(StdlibValue::Text(
            "invalid utf-8 sequence of 1 bytes from index 0".into()
        )))))
    );

    let hash = call_pure_stdlib("std.crypto.hash", &[StdlibValue::Bytes(b"ail".to_vec())])
        .expect("hash executes");
    let StdlibValue::Bytes(hash) = hash else {
        panic!("hash returns bytes")
    };
    assert_eq!(hash.len(), 32);
}

#[test]
fn executable_registry_covers_pure_and_effectful_modules() {
    let entries = stdlib_function_entries();

    for id in [
        "std.core.option.map",
        "std.collections.list.length",
        "std.text.normalize",
        "std.crypto.hash",
        "std.fs.read",
        "std.net.connect",
        "std.http.request",
        "std.process.spawn",
        "std.env.get",
        "std.log.log",
        "std.trace.span",
    ] {
        assert!(entries.iter().any(|entry| entry.id == id), "missing {id}");
    }
}

#[test]
fn v1_registry_with_functions_includes_executable_entries() {
    let registry = v1_registry_with_functions();

    let collections = registry
        .entries
        .iter()
        .find(|entry| entry.id.0 == "std.collections.list.length")
        .expect("collections executable entry");
    assert_eq!(collections.module_path, "std::collections");
    assert!(collections.capability_reqs.is_none());

    let fs_read = registry
        .entries
        .iter()
        .find(|entry| entry.id.0 == "std.fs.read")
        .expect("fs.read executable entry");
    assert_eq!(fs_read.module_path, "std::fs");
    assert_eq!(
        fs_read
            .capability_reqs
            .as_ref()
            .map(|reqs| reqs.caps.as_slice()),
        Some(&["file.read".to_string()][..])
    );

    assert_eq!(registry.validate(), Ok(()));
}
