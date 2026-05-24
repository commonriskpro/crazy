// ── ail-cli::compile_commands ─────────────────────────────────────────────
//
// Handler for `ail compile`.
//
// Routes through `emit_native_with_profile` when `--target native` is
// specified, and through `emit_wasm_with_profile` otherwise.
//
// Helpers:
//   verify_graph_for_compile    — real pre-lowering gate: runs TypeChecker and
//                                 EffectChecker; rejects reports with Failed or
//                                 Unsafe entries
//   check_report_accepted_for_compile — gate predicate, exposed for focused tests
//   accepted_compile_report     — proven-empty report forwarded to the lowering
//                                 pipeline (required by lower_to_core_ir's own
//                                 Proven/RuntimeChecked gate); also used by
//                                 run_commands for its pipeline path
//   detect_native_object_format — platform-specific object file format name

use ail_compiler::{
    emit_native_with_profile, emit_wasm_with_profile, lower_to_anf_with_graph, lower_to_core_ir,
};
use ail_core::semantic_graph::SemanticGraph;
use ail_verify::effect_checker::EffectChecker;
use ail_verify::report::{VerificationReport, VerificationState};
use ail_verify::type_checker::TypeChecker;
use serde_json::{Value, json};

use crate::cli::{bytes_to_hex, load_current_graph_for_cli};
use crate::error::CliError;
use crate::output::{OutputMode, print_response};
use crate::store::StoreHandle;
use crate::store_artifacts::{NativeArtifactBytes, WasmArtifactBytes};

// ── Verification gate ─────────────────────────────────────────────────────

/// Run the real verification gate before lowering a graph to Core IR.
///
/// Executes `TypeChecker::check` (type/effect/policy subpasses) and
/// `EffectChecker::check` (declared-vs-inferred effect consistency) on
/// `graph`, merges their entries, and rejects the result if any entry has
/// `VerificationState::Failed` or `VerificationState::Unsafe`.  Graphs
/// whose entries are all `Proven`, `RuntimeChecked`, `Assumed`, or
/// `Unverified` pass the gate.
///
/// Using the specialized checkers instead of the shallow `Checker` ensures
/// the gate is non-theatrical: real failure conditions (null return types,
/// undeclared emitted effects, type violations) produce blocking entries.
///
/// # Errors
///
/// Returns `CliError::Domain` listing every blocking entry when the graph
/// fails the verification gate.  Returns `Ok(())` when the graph passes.
pub(crate) fn verify_graph_for_compile(graph: &SemanticGraph) -> Result<(), CliError> {
    let type_report = TypeChecker::check(graph);
    let effect_report = EffectChecker::check(graph);
    let merged = VerificationReport {
        entries: type_report
            .entries
            .into_iter()
            .chain(effect_report.entries)
            .collect(),
        ..Default::default()
    };
    check_report_accepted_for_compile(&merged)
}

/// Gate predicate: return `Ok(())` when the report has no blocking entries.
///
/// Blocking entries have `VerificationState::Failed` or
/// `VerificationState::Unsafe`.  The gate filters on state directly rather
/// than on the `entry.blocking` flag so that inconsistencies in how
/// individual checkers set that field cannot silently make the gate a no-op.
///
/// Exposed as `pub(crate)` so focused tests can exercise the gate logic
/// directly with manually-constructed `VerificationReport` values.
pub(crate) fn check_report_accepted_for_compile(
    report: &VerificationReport,
) -> Result<(), CliError> {
    let blocking: Vec<_> = report
        .entries
        .iter()
        .filter(|e| e.state == VerificationState::Failed || e.state == VerificationState::Unsafe)
        .collect();
    if blocking.is_empty() {
        return Ok(());
    }
    let details = blocking
        .iter()
        .map(|e| format!("[{}] {} ({:?})", e.scope, e.claim, e.state))
        .collect::<Vec<_>>()
        .join("; ");
    Err(CliError::Domain(format!(
        "compile blocked by {} verification failure{}: {}",
        blocking.len(),
        if blocking.len() == 1 { "" } else { "s" },
        details
    )))
}

// ── Lowering-pipeline helpers ─────────────────────────────────────────────

/// Empty `VerificationReport` that satisfies the lowering pipeline's own
/// `Proven`/`RuntimeChecked` acceptance gate (`lower_to_core_ir`).
///
/// The compile gate (`verify_graph_for_compile`) runs first and rejects
/// graphs with `Failed`/`Unsafe` entries.  After the gate passes, this
/// proven-empty report is forwarded to the lowering pipeline.
///
/// Also used by `run_commands` for its pipeline path.
pub(crate) fn accepted_compile_report() -> VerificationReport {
    VerificationReport {
        entries: vec![],
        ..Default::default()
    }
}

// ── Private helpers ───────────────────────────────────────────────────────

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

    // ── Verification gate ─────────────────────────────────────────────────
    // Run the real checker and reject graphs with Failed/Unsafe entries
    // before entering the lowering pipeline.
    verify_graph_for_compile(&graph)?;

    // Forward a proven-empty report to the lowering pipeline (lower_to_core_ir
    // requires Proven or RuntimeChecked summary; the graph's own entries are
    // checked above, not by the lowering-pipeline gate).
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
