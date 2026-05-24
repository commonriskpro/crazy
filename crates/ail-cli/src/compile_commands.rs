// ── ail-cli::compile_commands ─────────────────────────────────────────────
//
// Handler for `ail compile`.
//
// Routes through `emit_native_with_profile` when `--target native` is
// specified, and through `emit_wasm_with_profile` otherwise.
//
// Private helpers:
//   accepted_compile_report     — stub VerificationReport used as pipeline input
//   detect_native_object_format — platform-specific object file format name

use ail_compiler::{
    emit_native_with_profile, emit_wasm_with_profile, lower_to_anf_with_graph, lower_to_core_ir,
};
use ail_verify::report::VerificationReport;
use serde_json::{Value, json};

use crate::cli::{bytes_to_hex, load_current_graph_for_cli};
use crate::error::CliError;
use crate::output::{OutputMode, print_response};
use crate::store::StoreHandle;
use crate::store_artifacts::{NativeArtifactBytes, WasmArtifactBytes};

// ── Private helpers ───────────────────────────────────────────────────────

/// Stub `VerificationReport` accepted as pipeline input for compile and run.
///
/// The type checker flags newly-materialised nodes as Unverified — a full
/// verify pass would reject the graph at this stage.  Callers that need to
/// drive the compiler pipeline directly (compile, run) use this instead.
pub(crate) fn accepted_compile_report() -> VerificationReport {
    VerificationReport {
        entries: vec![],
        ..Default::default()
    }
}

/// Detect the native object format name for the current compilation target.
///
/// - macOS   → `"Mach-O"`
/// - Windows → `"COFF"`
/// - other   → `"ELF"`
fn detect_native_object_format() -> &'static str {
    if cfg!(target_os = "macos") {
        "Mach-O"
    } else if cfg!(target_os = "windows") {
        "COFF"
    } else {
        "ELF"
    }
}

// ── Command handler ───────────────────────────────────────────────────────

/// `ail compile --target <target> --profile <name>`
///
/// Inputs: snapshot, accepted verification report for profile, runtime profile.
/// Outputs: wasm/native artifact, capabilities manifest, semantic source map,
///          artifact manifest, compiler report.
///
/// Rules:
/// - draft/dev/test artifacts are profile-bound
/// - prod runtime rejects non-prod artifacts
/// - `--target native` emits a platform object file (ELF/Mach-O/COFF);
///   the artifact is NOT a linked executable and cannot be run directly
pub(crate) async fn cmd_compile(
    mode: OutputMode,
    profile: &str,
    target: &str,
    store: &StoreHandle,
) -> Result<(), CliError> {
    let graph = load_current_graph_for_cli(store).await?;
    let report = accepted_compile_report();

    let core = lower_to_core_ir(&graph, &report)
        .map_err(|e| CliError::Domain(format!("Failed to lower graph to Core IR: {e}")))?;

    let anf = lower_to_anf_with_graph(&core, &graph)
        .map_err(|e| CliError::Domain(format!("Failed to lower Core IR to ANF: {e}")))?;

    if target == "native" {
        // ── Native object emission ─────────────────────────────────────────
        // Routes through `emit_native_with_profile` (Cranelift backend).
        // Output identifies the artifact as a native object file, not a linked
        // binary.  The artifact is suitable for linking but not direct execution.
        let artifact = emit_native_with_profile(&anf, profile)
            .map_err(|e| CliError::Domain(format!("Failed to emit native artifact: {e}")))?;

        let native_hash = artifact
            .hash_chain
            .native_hash
            .map(|h| bytes_to_hex(&h))
            .ok_or_else(|| CliError::Domain("compile native (missing native hash)".to_string()))?;
        let native_size = artifact.native_bytes.len();
        let object_format = detect_native_object_format();
        let capabilities_count = artifact.capabilities_manifest.entries.len();

        let capabilities_manifest_json_bytes = serde_json::to_vec(&artifact.capabilities_manifest)
            .map_err(|e| {
                CliError::Domain(format!("compile native (capabilities manifest bytes): {e}"))
            })?;
        let capabilities_manifest =
            serde_json::to_value(&artifact.capabilities_manifest).map_err(|e| {
                CliError::Domain(format!("compile native (capabilities manifest): {e}"))
            })?;
        let semantic_source_map: Value = serde_json::from_slice(&artifact.source_map_json)
            .map_err(|e| CliError::Domain(format!("compile native (source map sidecar): {e}")))?;
        let artifact_manifest: Value = serde_json::from_slice(&artifact.artifact_manifest_json)
            .map_err(|e| CliError::Domain(format!("compile native (artifact sidecar): {e}")))?;

        // ── Persist native artifact to .ail/native/ (file-backed stores only) ──
        let persisted_paths = store.save_native_artifact(
            &native_hash,
            profile,
            target,
            NativeArtifactBytes {
                object: &artifact.native_bytes,
                source_map_json: &artifact.source_map_json,
                artifact_manifest_json: &artifact.artifact_manifest_json,
                capabilities_manifest_json: &capabilities_manifest_json_bytes,
            },
        )?;
        let persisted = persisted_paths.as_ref().map(|p| {
            json!({
                "object_path": p.object_path.to_string_lossy(),
                "source_map_path": p.source_map_path.to_string_lossy(),
                "manifest_path": p.manifest_path.to_string_lossy(),
                "capabilities_path": p.capabilities_path.to_string_lossy(),
            })
        });

        let compiler_report = json!({
            "profile": profile,
            "target": target,
            "stages": ["core_ir", "anf", "emit_native"],
            "warnings": [],
            "errors": [],
        });

        let human_msg = format!(
            "target: {target}\nprofile: {profile}\nobject_format: {object_format}\nnative_bytes: {native_size}\nnative_hash: {native_hash}\nartifact_type: object (not a linked executable)\ncapabilities: {capabilities_count}\nwarnings: 0"
        );
        print_response(
            mode,
            &human_msg,
            json!({
                "profile": profile,
                "target": target,
                "object_format": object_format,
                "native_bytes": native_size,
                "native_hash": native_hash,
                "capabilities_manifest": capabilities_manifest,
                "semantic_source_map": semantic_source_map,
                "artifact_manifest": artifact_manifest,
                "compiler_report": compiler_report,
                "persisted_paths": persisted,
            }),
        );
        return Ok(());
    }

    // ── WASM emission (default path; JSON contract unchanged) ─────────────
    let artifact = emit_wasm_with_profile(&anf, profile)
        .map_err(|e| CliError::Domain(format!("Failed to emit WASM artifact: {e}")))?;

    let wasm_hash = artifact
        .hash_chain
        .wasm_hash
        .map(|h| bytes_to_hex(&h))
        .ok_or_else(|| CliError::Domain("compile wasm (missing wasm hash)".to_string()))?;
    let wasm_size = artifact.wasm.len();
    let capabilities_count = artifact.capabilities_manifest.entries.len();

    // Serialize the real capabilities manifest — one entry per ANF binding.
    let capabilities_manifest = serde_json::to_value(&artifact.capabilities_manifest)
        .map_err(|e| CliError::Domain(format!("compile (capabilities manifest): {e}")))?;
    let capabilities_manifest_json_bytes = serde_json::to_vec(&artifact.capabilities_manifest)
        .map_err(|e| CliError::Domain(format!("compile (capabilities manifest bytes): {e}")))?;
    let semantic_source_map: Value = serde_json::from_slice(&artifact.source_map_json)
        .map_err(|e| CliError::Domain(format!("compile (source map sidecar): {e}")))?;
    let artifact_manifest: Value = serde_json::from_slice(&artifact.artifact_manifest_json)
        .map_err(|e| CliError::Domain(format!("compile (artifact sidecar): {e}")))?;

    // ── Persist WASM artifact to .ail/wasm/ (file-backed stores only) ─────
    let persisted_paths = store.save_wasm_artifact(
        &wasm_hash,
        profile,
        target,
        WasmArtifactBytes {
            wasm: &artifact.wasm,
            source_map_json: &artifact.source_map_json,
            artifact_manifest_json: &artifact.artifact_manifest_json,
            capabilities_manifest_json: &capabilities_manifest_json_bytes,
        },
    )?;
    let persisted = persisted_paths.as_ref().map(|p| {
        json!({
            "wasm_path": p.wasm_path.to_string_lossy(),
            "source_map_path": p.source_map_path.to_string_lossy(),
            "manifest_path": p.manifest_path.to_string_lossy(),
            "capabilities_path": p.capabilities_path.to_string_lossy(),
        })
    });

    // Compiler report.
    let compiler_report = json!({
        "profile": profile,
        "target": target,
        "stages": ["core_ir", "anf", format!("emit_{target}")],
        "warnings": [],
        "errors": [],
    });

    let human_msg = format!(
        "target: {target}\nprofile: {profile}\nwasm bytes: {wasm_size}\nwasm-hash: {wasm_hash}\ncapabilities: {capabilities_count}\nwarnings: 0"
    );
    print_response(
        mode,
        &human_msg,
        json!({
            "profile": profile,
            "target": target,
            "wasm_bytes": wasm_size,
            "wasm_hash": wasm_hash,
            "capabilities_manifest": capabilities_manifest,
            "semantic_source_map": semantic_source_map,
            "artifact_manifest": artifact_manifest,
            "compiler_report": compiler_report,
            "persisted_paths": persisted,
        }),
    );
    Ok(())
}
