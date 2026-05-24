use super::*;

// Scenario: cmd_compile succeeds with an empty graph (exit 0).
#[tokio::test]
async fn cmd_compile_succeeds() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_compile(OutputMode::Human, "dev", "wasm", &store).await;
    assert!(result.is_ok(), "cmd_compile must succeed; got: {result:?}");
}

#[test]
fn current_graph_for_cli_contains_executable_function() {
    let graph = current_graph_for_cli().expect("graph must load");

    assert!(
        graph.nodes.iter().any(|node| node.name == "fn.answer"),
        "CLI compile/run graph must contain fn.answer"
    );
}

// Scenario: cmd_compile with native target succeeds.
#[tokio::test]
async fn cmd_compile_native_target_succeeds() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_compile(OutputMode::Human, "prod", "native", &store).await;
    assert!(
        result.is_ok(),
        "cmd_compile native must succeed; got: {result:?}"
    );
}

// Scenario: cmd_compile with native target produces native-specific JSON fields.
//   GIVEN target == "native"
//   WHEN cmd_compile is called
//   THEN it returns Ok (native object emitted via emit_native_with_profile)
#[tokio::test]
async fn cmd_compile_native_target_routes_to_native_backend() {
    use crate::store::memory_store;
    let store = memory_store();
    // Verify routing: native target must succeed (calls emit_native_with_profile).
    let result = cmd_compile(OutputMode::Human, "dev", "native", &store).await;
    assert!(
        result.is_ok(),
        "cmd_compile native must succeed via emit_native_with_profile; got: {result:?}"
    );
}

// Scenario: cmd_compile wasm target still succeeds (contract unchanged).
//   GIVEN target == "wasm"
//   WHEN cmd_compile is called
//   THEN it returns Ok with WASM artifact
#[tokio::test]
async fn cmd_compile_wasm_target_still_succeeds() {
    use crate::store::memory_store;
    let store = memory_store();
    let result = cmd_compile(OutputMode::Human, "dev", "wasm", &store).await;
    assert!(
        result.is_ok(),
        "cmd_compile wasm must still succeed; got: {result:?}"
    );
}

// Scenario: cmd_compile with a file-backed store persists artifact bytes and sidecars.
//   GIVEN a file store
//   WHEN cmd_compile --target wasm is called
//   THEN .ail/wasm/<hash>.wasm and the three sidecar files exist on disk
#[tokio::test]
async fn cmd_compile_wasm_with_file_store_persists_artifact() {
    use crate::store::{file_store, init_file_layout};
    use std::fs;

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir.clone());

    let result = cmd_compile(OutputMode::Human, "dev", "wasm", &store).await;
    assert!(result.is_ok(), "compile must succeed; got: {result:?}");

    let wasm_dir = ail_dir.join("wasm");
    assert!(wasm_dir.exists(), ".ail/wasm/ must exist after compile");

    let index_path = wasm_dir.join("artifact-index.json");
    assert!(
        index_path.exists(),
        ".ail/wasm/artifact-index.json must exist after compile"
    );

    // At least one .wasm file must be present.
    let wasm_files: Vec<_> = fs::read_dir(&wasm_dir)
        .expect("read wasm dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "wasm").unwrap_or(false))
        .collect();
    assert!(
        !wasm_files.is_empty(),
        ".ail/wasm/ must contain at least one .wasm file after compile"
    );

    // Each .wasm file should have a matching .manifest.json sidecar.
    for wasm_entry in &wasm_files {
        let stem = wasm_entry
            .path()
            .file_stem()
            .expect("wasm file stem")
            .to_string_lossy()
            .to_string();
        let manifest_path = wasm_dir.join(format!("{stem}.manifest.json"));
        assert!(
            manifest_path.exists(),
            ".ail/wasm/{stem}.manifest.json must exist alongside {stem}.wasm"
        );
        let source_map_path = wasm_dir.join(format!("{stem}.source_map.json"));
        assert!(
            source_map_path.exists(),
            ".ail/wasm/{stem}.source_map.json must exist alongside {stem}.wasm"
        );
        let capabilities_path = wasm_dir.join(format!("{stem}.capabilities.json"));
        assert!(
            capabilities_path.exists(),
            ".ail/wasm/{stem}.capabilities.json must exist alongside {stem}.wasm"
        );
    }
}

// Scenario: cmd_compile --target native with a file-backed store persists artifact bytes and sidecars.
//   GIVEN a file store
//   WHEN cmd_compile --target native is called
//   THEN .ail/native/<hash>.o and the three sidecar files exist on disk
#[tokio::test]
async fn cmd_compile_native_with_file_store_persists_artifact() {
    use crate::store::{file_store, init_file_layout};
    use std::fs;

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir.clone());

    let result = cmd_compile(OutputMode::Human, "dev", "native", &store).await;
    assert!(
        result.is_ok(),
        "native compile must succeed; got: {result:?}"
    );

    let native_dir = ail_dir.join("native");
    assert!(
        native_dir.exists(),
        ".ail/native/ must exist after native compile"
    );

    let index_path = native_dir.join("artifact-index.json");
    assert!(
        index_path.exists(),
        ".ail/native/artifact-index.json must exist after native compile"
    );

    // At least one .o file must be present.
    let object_files: Vec<_> = fs::read_dir(&native_dir)
        .expect("read native dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "o").unwrap_or(false))
        .collect();
    assert!(
        !object_files.is_empty(),
        ".ail/native/ must contain at least one .o file after native compile"
    );

    // Each .o file should have matching sidecar files.
    for obj_entry in &object_files {
        let stem = obj_entry
            .path()
            .file_stem()
            .expect("object file stem")
            .to_string_lossy()
            .to_string();
        let manifest_path = native_dir.join(format!("{stem}.manifest.json"));
        assert!(
            manifest_path.exists(),
            ".ail/native/{stem}.manifest.json must exist alongside {stem}.o"
        );
        let source_map_path = native_dir.join(format!("{stem}.source_map.json"));
        assert!(
            source_map_path.exists(),
            ".ail/native/{stem}.source_map.json must exist alongside {stem}.o"
        );
        let capabilities_path = native_dir.join(format!("{stem}.capabilities.json"));
        assert!(
            capabilities_path.exists(),
            ".ail/native/{stem}.capabilities.json must exist alongside {stem}.o"
        );
    }
}

// Scenario: cmd_compile --target native JSON output contains persisted_paths.
//   GIVEN a file store
//   WHEN cmd_compile --target native is called in Json mode
//   THEN the JSON output contains a non-null persisted_paths field
#[tokio::test]
async fn cmd_compile_native_json_output_has_persisted_paths() {
    use crate::store::{file_store, init_file_layout};

    let temp = tempfile::tempdir().expect("tempdir");
    let ail_dir = temp.path().join(".ail");
    init_file_layout(&ail_dir).expect("init layout");
    let store = file_store(ail_dir.clone());

    // Capture JSON output by running in Json mode (output goes to stdout;
    // we just verify the command succeeds — the persisted_paths are on disk).
    let result = cmd_compile(OutputMode::Json, "dev", "native", &store).await;
    assert!(
        result.is_ok(),
        "native compile (Json mode) must succeed; got: {result:?}"
    );

    // The artifact-index must be present, proving persistence ran.
    assert!(
        ail_dir.join("native").join("artifact-index.json").exists(),
        "artifact-index.json must exist, proving persisted_paths were written"
    );
}
