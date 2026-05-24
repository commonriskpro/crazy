// ── ail-compiler::native_object_smoke_tests ──────────────────────────────
//
// Native-1 binary/object smoke path.
//
// # Honest scope declaration
//
// `emit_native` produces a platform-native OBJECT FILE (ELF on Linux,
// Mach-O on macOS, COFF on Windows) — NOT a linked, runnable executable.
//
// Limitations of the current subset (Phase 17 / Native-1):
// - No linker invocation. A system linker (`cc -o prog prog.o`) is required
//   to produce a runnable binary.
// - No runtime host: imported stubs (`host_call`, `__ail_malloc`,
//   `ail_runtime_call`) must be supplied at link time.
// - Phase 8 expression lowering covers arithmetic, control-flow, loops,
//   match, text literals, records/variants/lists/tuples, EffectCall, and
//   Lambda (no closure capture). Concurrency, dynamic dispatch, resource
//   lifecycle, and channel primitives dispatch via `ail_runtime_call`.
// - No self-hosting: the compiler itself is not compiled by ail-compiler.
//
// Path to real binary / self-hosting:
// 1. Phase 9+: closure capture, heap model (GC/RC), and concurrency runtime.
// 2. Phase 9: link-time ABI: runtime host provides `host_call`/`__ail_malloc`.
// 3. Phase 10+: linker integration via `cc` or `lld` system call.
// 4. Self-hosting: once the full language surface compiles, bootstrap ail-compiler itself.
//
// # What these tests prove
//
// - `emit_native` deterministically emits a structurally valid native object
//   for the current ANF executable subset.
// - The emitted bytes have the correct platform-native object file magic.
// - The `NativeArtifact.provenance` map covers every binding with correct
//   `NodeRef` → byte-offset entries.
// - The sealed `native_hash` matches the explicit recomputation formula.
// - `emit_native_with_profile("prod")` and `"critical"` enforce Wave 6B
//   provenance coverage (reject artifacts whose source map lacks `change_set`).
// - Simple arithmetic expression bodies compile to real Cranelift IR, producing
//   non-stub object bytes distinct from `Placeholder` trap stubs.
//
// These tests are intentionally named *_object_smoke_* to distinguish them
// from a hypothetical future *_binary_smoke_* suite that would test linked
// executables produced by a linker invocation.

use ail_compiler::{
    AnfBinding, AnfExpr, AnfIr, CompileError, SourceMap,
    anf::ANF_SCHEMA_VERSION,
    core_ir::{LiteralValue, StageHashes},
    emit_native, emit_native_with_profile,
    lower::{lower_to_anf, lower_to_core_ir},
};
use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::report::VerificationReport;

// ── helpers ──────────────────────────────────────────────────────────────

fn proven_report() -> VerificationReport {
    VerificationReport {
        entries: vec![],
        ..Default::default()
    }
}

fn graph_with_n_functions(n: usize) -> SemanticGraph {
    SemanticGraph {
        nodes: (0..n)
            .map(|i| GraphNode::new(NodeRef(i as u32), NodeKind::Function, format!("fn_{i}")))
            .collect(),
        edges: vec![],
    }
}

fn anf_for_n(n: usize) -> AnfIr {
    let graph = graph_with_n_functions(n);
    let core = lower_to_core_ir(&graph, &proven_report()).expect("lower_to_core_ir");
    lower_to_anf(&core).expect("lower_to_anf")
}

/// Build a minimal ANF with a sealed `anf_ir_hash` from a single binding.
///
/// Suitable for provenance-gate tests that need a bare `AnfIr` without
/// going through the full `lower_to_anf` pipeline.
fn sealed_anf_single(binding: AnfBinding) -> AnfIr {
    AnfIr {
        schema_version: ANF_SCHEMA_VERSION,
        source_map: SourceMap::from_bindings(std::slice::from_ref(&binding)),
        bindings: vec![binding],
        stage_hashes: StageHashes {
            graph_snapshot_hash: [0u8; 32],
            verification_report_hash: [0u8; 32],
            core_ir_hash: [1u8; 32],
            anf_ir_hash: Some([2u8; 32]),
            wasm_hash: None,
            native_hash: None,
            source_map_hash: None,
            artifact_manifest_hash: None,
        },
    }
}

/// Build a prod-quality ANF with `change_set` provenance in every source map entry.
///
/// Used for Wave 6B provenance gate smoke tests.
fn prod_anf_for_n(n: usize) -> AnfIr {
    let mut anf = anf_for_n(n);
    // Inject synthetic `change_set` into every source map entry so the prod
    // gate accepts the artifact.
    for (i, entry) in anf.source_map.entries.iter_mut().enumerate() {
        entry.change_set = Some(format!("change.fn_{i}"));
    }
    anf
}

/// Return the expected object file magic bytes for the current platform.
///
/// Returns a slice whose values must prefix the emitted object bytes for the
/// artifact to be a structurally valid native object file.
///
/// This check is intentionally lenient — it only validates the magic header,
/// not the full ELF/Mach-O/COFF structure.  Full structural validation would
/// require the `object` crate or similar library and is out of scope here.
fn native_object_magic() -> &'static [u8] {
    // ELF magic: 0x7F 'E' 'L' 'F'
    #[cfg(target_os = "linux")]
    return &[0x7F, 0x45, 0x4C, 0x46];

    // Mach-O 64-bit little-endian: CF FA ED FE
    #[cfg(target_os = "macos")]
    return &[0xCF, 0xFA, 0xED, 0xFE];

    // COFF does not have a universal magic; accept any non-empty header.
    #[cfg(target_os = "windows")]
    return &[];

    // Unknown platform: no platform-specific magic to check.
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    return &[];
}

fn assert_native_object_magic(bytes: &[u8]) {
    let magic = native_object_magic();
    if magic.is_empty() {
        assert!(
            !bytes.is_empty(),
            "native object smoke: emitted bytes must be non-empty"
        );
        return;
    }
    assert!(
        bytes.len() >= magic.len(),
        "native object smoke: emitted bytes too short to contain magic (got {} bytes)",
        bytes.len()
    );
    assert_eq!(
        &bytes[..magic.len()],
        magic,
        "native object smoke: magic header mismatch — emitted bytes do not start with \
         the expected platform-native object file magic.\n\
         Expected: {magic:02X?}\n\
         Got:      {:02X?}",
        &bytes[..magic.len()]
    );
}

// ── Smoke: object file magic bytes ────────────────────────────────────────

/// Object smoke: 1-binding ANF emits bytes that start with the platform-native
/// object file magic (ELF on Linux, Mach-O on macOS, COFF on Windows).
///
/// This is the primary structural assertion: the emitted artifact is a
/// PLATFORM-NATIVE OBJECT FILE, not a linked executable.
#[test]
fn object_smoke_magic_bytes_are_valid_for_platform() {
    let anf = anf_for_n(1);
    let artifact = emit_native(&anf).expect("emit_native");
    assert_native_object_magic(&artifact.native_bytes);
}

/// Object smoke: empty ANF emits a valid (though function-less) object file.
///
/// The object module produces at minimum an ELF/Mach-O/COFF header even when
/// there are no functions.  The magic bytes must still be correct.
#[test]
fn object_smoke_empty_anf_emits_valid_object_header() {
    let anf = anf_for_n(0);
    let artifact = emit_native(&anf).expect("emit_native");
    assert_native_object_magic(&artifact.native_bytes);
}

/// Object smoke: 5-binding ANF emits a valid object file.
///
/// Triangulates that the magic bytes check generalises beyond N=1.
#[test]
fn object_smoke_five_binding_anf_emits_valid_object_header() {
    let anf = anf_for_n(5);
    let artifact = emit_native(&anf).expect("emit_native");
    assert_native_object_magic(&artifact.native_bytes);
}

// ── Smoke: determinism with provenance metadata ───────────────────────────

/// Object smoke: same ANF → byte-identical native_bytes and native_hash.
///
/// Determinism is required by the compiler.md hash/provenance chain contract:
/// "If any upstream hash changes, downstream artifacts must be regenerated."
/// Determinism is the dual: if nothing changes, artifacts must be identical.
#[test]
fn object_smoke_determinism_native_bytes_and_hash() {
    let anf = anf_for_n(3);
    let a1 = emit_native(&anf).expect("first emit");
    let a2 = emit_native(&anf).expect("second emit");

    assert_eq!(
        a1.native_bytes, a2.native_bytes,
        "object smoke: native_bytes must be byte-identical across calls with the same AnfIr"
    );
    assert_eq!(
        a1.hash_chain.native_hash, a2.hash_chain.native_hash,
        "object smoke: native_hash must be identical across calls with the same AnfIr"
    );
}

/// Object smoke: provenance map has one entry per binding with correct NodeRefs.
///
/// The `NativeArtifact.provenance: BTreeMap<NodeRef, u64>` maps every binding's
/// `source_ref` to its byte offset in the object file code section.  This is
/// the primary provenance contract for the native backend.
#[test]
fn object_smoke_provenance_has_correct_node_refs() {
    let n = 4;
    let anf = anf_for_n(n);
    let artifact = emit_native(&anf).expect("emit_native");

    assert_eq!(
        artifact.provenance.len(),
        n,
        "object smoke: provenance must have {n} entries for {n}-binding AnfIr"
    );

    for i in 0..n as u32 {
        assert!(
            artifact.provenance.contains_key(&NodeRef(i)),
            "object smoke: provenance must contain NodeRef({i})"
        );
    }
}

/// Object smoke: provenance offsets are monotonically non-decreasing.
///
/// Functions are emitted in binding order; each successive function's offset
/// must be >= the previous one.
#[test]
fn object_smoke_provenance_offsets_are_non_decreasing() {
    let anf = anf_for_n(4);
    let artifact = emit_native(&anf).expect("emit_native");

    let offsets: Vec<u64> = anf
        .bindings
        .iter()
        .map(|b| artifact.provenance[&b.source_ref])
        .collect();

    for w in offsets.windows(2) {
        assert!(
            w[1] >= w[0],
            "object smoke: provenance offsets must be non-decreasing: {w:?}"
        );
    }
}

/// Object smoke: sealed native_hash matches explicit blake3 recomputation.
///
/// Verifies the hash chain formula:
///   `native_hash = blake3(anf_ir_hash || native_bytes)`
#[test]
fn object_smoke_native_hash_matches_explicit_recomputation() {
    use ail_compiler::hash::hash_with_parent;

    let anf = anf_for_n(2);
    let artifact = emit_native(&anf).expect("emit_native");

    let anf_ir_hash = anf
        .stage_hashes
        .anf_ir_hash
        .expect("anf_ir_hash must be sealed by lower_to_anf");

    let expected = hash_with_parent(&anf_ir_hash, &artifact.native_bytes);

    assert_eq!(
        artifact.hash_chain.native_hash,
        Some(expected),
        "object smoke: native_hash must equal blake3(anf_ir_hash || native_bytes)"
    );
}

/// Object smoke: source map has one entry per binding with native_offset populated.
///
/// The source map is the Wave 6B provenance carrier: every backend must populate
/// native_offset so the semantic provenance chain is complete end-to-end.
#[test]
fn object_smoke_source_map_has_native_offset_for_every_binding() {
    let n = 3;
    let anf = anf_for_n(n);
    let artifact = emit_native(&anf).expect("emit_native");

    assert_eq!(
        artifact.source_map.entries.len(),
        n,
        "object smoke: source map must have {n} entries"
    );

    for (i, entry) in artifact.source_map.entries.iter().enumerate() {
        assert!(
            entry.native_offset.is_some(),
            "object smoke: source map entry {i} must have native_offset populated"
        );
    }
}

/// Object smoke: source_map_hash is sealed after emit_native.
///
/// The source_map_hash covers all binding offsets and provenance fields.
/// Any mutation of the source map changes this hash, invalidating downstream
/// manifests.
#[test]
fn object_smoke_source_map_hash_is_sealed() {
    let anf = anf_for_n(2);
    let artifact = emit_native(&anf).expect("emit_native");
    assert!(
        artifact.hash_chain.source_map_hash.is_some(),
        "object smoke: source_map_hash must be Some after emit_native"
    );
}

/// Object smoke: artifact_manifest_hash is sealed after emit_native.
#[test]
fn object_smoke_artifact_manifest_hash_is_sealed() {
    let anf = anf_for_n(1);
    let artifact = emit_native(&anf).expect("emit_native");
    assert!(
        artifact.hash_chain.artifact_manifest_hash.is_some(),
        "object smoke: artifact_manifest_hash must be Some after emit_native"
    );
}

/// Object smoke: JSON sidecars are non-empty and parseable.
///
/// `source_map_json` and `artifact_manifest_json` are the on-disk sidecars
/// written by callers.  They must be valid UTF-8 JSON.
#[test]
fn object_smoke_json_sidecars_are_valid_json() {
    let anf = anf_for_n(2);
    let artifact = emit_native(&anf).expect("emit_native");

    let sm_json = std::str::from_utf8(&artifact.source_map_json)
        .expect("source_map_json must be valid UTF-8");
    let am_json = std::str::from_utf8(&artifact.artifact_manifest_json)
        .expect("artifact_manifest_json must be valid UTF-8");

    // Validate by parsing.
    let sm_parsed: serde_json::Value =
        serde_json::from_str(sm_json).expect("source_map_json must be valid JSON");
    let am_parsed: serde_json::Value =
        serde_json::from_str(am_json).expect("artifact_manifest_json must be valid JSON");

    assert!(
        sm_parsed.is_object(),
        "source_map_json must deserialize to a JSON object"
    );
    assert!(
        am_parsed.is_object(),
        "artifact_manifest_json must deserialize to a JSON object"
    );
}

// ── Smoke: arithmetic expression bodies (not trap stubs) ──────────────────

/// Object smoke: arithmetic ANF produces different bytes than a Placeholder.
///
/// Proves that the current subset emits REAL Cranelift IR for `i64.add`,
/// not a generic trap stub.  This is the core "executable subset" claim.
#[test]
fn object_smoke_arithmetic_differs_from_placeholder() {
    let arithmetic_anf = sealed_anf_single(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_add".to_string(),
        expr: AnfExpr::Let {
            name: "x".to_string(),
            value: Box::new(AnfExpr::Literal(LiteralValue::Int(1))),
            body: Box::new(AnfExpr::Let {
                name: "y".to_string(),
                value: Box::new(AnfExpr::Literal(LiteralValue::Int(2))),
                body: Box::new(AnfExpr::Call {
                    func: "i64.add".to_string(),
                    args: vec!["x".to_string(), "y".to_string()],
                }),
            }),
        },
    });

    let placeholder_anf = sealed_anf_single(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_add".to_string(),
        expr: AnfExpr::Placeholder,
    });

    let art_arith = emit_native(&arithmetic_anf).expect("arithmetic emit");
    let art_placeholder = emit_native(&placeholder_anf).expect("placeholder emit");

    assert_ne!(
        art_arith.native_bytes, art_placeholder.native_bytes,
        "object smoke: i64.add must produce different object bytes than Placeholder"
    );
}

/// Object smoke: integer literal compiles to non-empty object.
#[test]
fn object_smoke_int_literal_emits_valid_object() {
    let anf = sealed_anf_single(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_const".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Int(42)),
    });

    let artifact = emit_native(&anf).expect("emit_native");
    assert_native_object_magic(&artifact.native_bytes);
    assert!(
        artifact.hash_chain.native_hash.is_some(),
        "object smoke: int literal must seal native_hash"
    );
}

/// Object smoke: bool literal emits valid object (non-stub I8 path).
#[test]
fn object_smoke_bool_literal_emits_valid_object() {
    let anf = sealed_anf_single(AnfBinding {
        source_ref: NodeRef(0),
        name: "fn_bool".to_string(),
        expr: AnfExpr::Literal(LiteralValue::Bool(true)),
    });

    let artifact = emit_native(&anf).expect("emit_native");
    assert_native_object_magic(&artifact.native_bytes);
}

// ── Smoke: Wave 6B provenance gate (prod/critical profiles) ───────────────
//
// Wave 6B hardening: prod/critical artifacts must carry `change_set`
// provenance in every source map entry.  The gate is enforced by
// `SourceMap::validate_required_provenance` called inside `emit_native_with_profile`.

/// Object smoke: emit_native_with_profile("prod") rejects missing change_set.
///
/// Wave 6B gate: prod artifacts without `change_set` provenance must be rejected.
/// This is the primary production safety guarantee.
#[test]
fn object_smoke_prod_profile_rejects_missing_change_set() {
    let anf = anf_for_n(1); // source map has no change_set
    let result = emit_native_with_profile(&anf, "prod");

    assert!(
        matches!(
            result,
            Err(CompileError::MissingProvenanceMetadata {
                field: "change_set",
                ..
            })
        ),
        "object smoke: prod profile must reject missing change_set provenance, got {result:?}"
    );
}

/// Object smoke: emit_native_with_profile("critical") rejects missing change_set.
#[test]
fn object_smoke_critical_profile_rejects_missing_change_set() {
    let anf = anf_for_n(1);
    let result = emit_native_with_profile(&anf, "critical");

    assert!(
        matches!(
            result,
            Err(CompileError::MissingProvenanceMetadata {
                field: "change_set",
                ..
            })
        ),
        "object smoke: critical profile must reject missing change_set, got {result:?}"
    );
}

/// Object smoke: emit_native_with_profile("prod") accepts artifact with change_set.
///
/// The prod gate must pass when every source map entry carries a non-empty
/// `change_set`.  This proves the Wave 6B gate does not block valid prod artifacts.
#[test]
fn object_smoke_prod_profile_accepts_complete_change_set_provenance() {
    let anf = prod_anf_for_n(2);
    let result = emit_native_with_profile(&anf, "prod");

    assert!(
        result.is_ok(),
        "object smoke: prod profile must accept artifact with complete change_set, got {result:?}"
    );
    let artifact = result.unwrap();
    assert_native_object_magic(&artifact.native_bytes);
}

/// Object smoke: emit_native_with_profile("unspecified") is permissive.
///
/// Non-prod profiles do not enforce the change_set gate.
/// An ANF without change_set provenance must succeed for "unspecified".
#[test]
fn object_smoke_unspecified_profile_is_permissive() {
    let anf = anf_for_n(1); // no change_set
    let result = emit_native_with_profile(&anf, "unspecified");

    assert!(
        result.is_ok(),
        "object smoke: unspecified profile must not require change_set, got {result:?}"
    );
}

/// Object smoke: emit_native (default profile = "unspecified") is permissive.
#[test]
fn object_smoke_default_emit_native_is_permissive() {
    let anf = anf_for_n(1);
    let result = emit_native(&anf);
    assert!(
        result.is_ok(),
        "object smoke: emit_native (default profile) must not require change_set, got {result:?}"
    );
}

// ── Smoke: source_map_json provenance fields ──────────────────────────────

/// Object smoke: source_map_json contains native_offset for every binding.
///
/// The JSON sidecar must retain the native byte offsets so downstream tools
/// (debuggers, profilers, LLM repair context) can map runtime errors back to
/// source-graph nodes.
#[test]
fn object_smoke_source_map_json_contains_native_offsets() {
    let n = 2;
    let anf = anf_for_n(n);
    let artifact = emit_native(&anf).expect("emit_native");

    let sm_json =
        std::str::from_utf8(&artifact.source_map_json).expect("source_map_json must be UTF-8");
    let sm: serde_json::Value =
        serde_json::from_str(sm_json).expect("source_map_json must be valid JSON");

    let entries = sm["entries"].as_array().expect("entries must be an array");
    assert_eq!(entries.len(), n, "source_map_json must have {n} entries");

    for (i, entry) in entries.iter().enumerate() {
        assert!(
            !entry["native_offset"].is_null(),
            "source_map_json entry {i} must have a non-null native_offset"
        );
    }
}

// ── Smoke: SourceMapEntry with provenance fields in custom ANF ────────────

/// Object smoke: source map entries with injected `change_set` survive emit_native
/// with prod profile, and native_offset is populated in the output.
///
/// This verifies the Wave 6B → native emit path end-to-end: if the source map
/// carries `change_set` provenance, the prod gate passes and the native backend
/// populates `native_offset` for every entry.
#[test]
fn object_smoke_change_set_provenance_in_source_map_survives_emit_native_prod() {
    // Build an ANF via the pipeline, then inject change_set into the source map
    // to simulate what lower_to_anf_with_graph does for graph-annotated nodes.
    let mut anf = anf_for_n(2);
    for (i, entry) in anf.source_map.entries.iter_mut().enumerate() {
        entry.change_set = Some(format!("change.smoke_{i}"));
    }

    let artifact = emit_native_with_profile(&anf, "prod")
        .expect("object smoke: prod emit must succeed when change_set is populated");

    // Every source map entry must have native_offset after emit_native.
    for (i, entry) in artifact.source_map.entries.iter().enumerate() {
        assert!(
            entry.native_offset.is_some(),
            "object smoke: source map entry {i} must have native_offset after emit_native"
        );
        assert_eq!(
            entry.change_set.as_deref(),
            Some(format!("change.smoke_{i}").as_str()),
            "object smoke: change_set must be preserved in source map after emit_native"
        );
    }

    assert_native_object_magic(&artifact.native_bytes);
}
