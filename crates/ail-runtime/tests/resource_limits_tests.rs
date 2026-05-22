// ── ail-runtime::resource_limits_tests ───────────────────────────────────
//
// Integration tests for G19: ResourceLimits enforcement in Wasmtime.
//
// Spec scenarios covered:
//  RL1 — Module with a start function that loops infinitely is trapped when
//        max_fuel is set to a small value.
//  RL2 — Module that fits within a generous fuel budget instantiates successfully.
//  RL3 — Module within limits (no fuel cap) succeeds with None max_fuel.
//  RL4 — Profile with max_memory_bytes smaller than requested memory growth
//        causes the grow to trap.
//  RL5 — Module that does not grow memory succeeds with a tight memory cap.

use wasm_encoder::{
    BlockType, CodeSection, Function, FunctionSection, Instruction, MemorySection, MemoryType,
    Module, StartSection, TypeSection,
};

use ail_runtime::{
    CapabilityManifest, PreflightFailure, ResourceLimits, RuntimeError, RuntimeHost,
    RuntimeProfile, blake3_hex_of,
};

// ── WASM builders ────────────────────────────────────────────────────────

/// Build a WASM module with a start function containing an infinite loop.
///
/// The loop executes `nop` instructions in a tight `block { loop { nop*;
/// br 0 } }` structure, which never terminates and will exhaust any finite
/// fuel budget.
fn wasm_infinite_loop_start() -> Vec<u8> {
    let mut module = Module::new();

    // Type section: define () -> () for the start function.
    let mut types = TypeSection::new();
    types.ty().function(vec![], vec![]);
    module.section(&types);

    // Function section: one function of type 0.
    let mut funcs = FunctionSection::new();
    funcs.function(0);
    module.section(&funcs);

    // Start section: must appear before the Code section in the WASM binary.
    module.section(&StartSection { function_index: 0 });

    // Code section: infinite loop in the start function.
    let mut code = CodeSection::new();
    let mut f = Function::new(vec![]);
    f.instruction(&Instruction::Loop(BlockType::Empty));
    f.instruction(&Instruction::Nop);
    f.instruction(&Instruction::Br(0)); // unconditional branch back to loop label
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::End);
    code.function(&f);
    module.section(&code);

    module.finish()
}

/// Build a minimal valid WASM module (magic + version, no sections).
///
/// Contains no start function, no memory, no code — instantiates instantly
/// and consumes essentially zero fuel.
fn wasm_minimal() -> Vec<u8> {
    vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
}

/// Build a WASM module with a start function that calls `memory.grow` to
/// allocate `pages` additional pages.
///
/// Uses Wasm page size (64 KiB).  If the `StoreLimits` resource limiter
/// denies the growth, Wasmtime traps at the `memory.grow` instruction
/// (because we set `trap_on_grow_failure(true)`).
fn wasm_memory_grow_start(pages: u32) -> Vec<u8> {
    let mut module = Module::new();

    // Type section: () -> ()
    let mut types = TypeSection::new();
    types.ty().function(vec![], vec![]);
    module.section(&types);

    // Function section: one function.
    let mut funcs = FunctionSection::new();
    funcs.function(0);
    module.section(&funcs);

    // Memory section: initial = 1 page (64 KiB), no maximum declared in WASM.
    let mut mems = MemorySection::new();
    mems.memory(MemoryType {
        minimum: 1,
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });
    module.section(&mems);

    // Start section: must appear before the Code section in the WASM binary.
    module.section(&StartSection { function_index: 0 });

    // Code section: push const pages, memory.grow, drop result, end.
    let mut code = CodeSection::new();
    let mut f = Function::new(vec![]);
    f.instruction(&Instruction::I32Const(pages as i32));
    f.instruction(&Instruction::MemoryGrow(0));
    f.instruction(&Instruction::Drop);
    f.instruction(&Instruction::End);
    code.function(&f);
    module.section(&code);

    module.finish()
}

// ── helpers ──────────────────────────────────────────────────────────────

fn matching_profile(
    wasm: &[u8],
    manifest: &CapabilityManifest,
    limits: ResourceLimits,
) -> RuntimeProfile {
    let module_hash = blake3_hex_of(wasm);
    let manifest_hash = manifest
        .blake3_hex()
        .expect("manifest CBOR hash must succeed");
    RuntimeProfile::new(
        "resource-limits-test-profile".to_string(),
        module_hash,
        "a".repeat(64),
        manifest_hash,
        vec![],
        limits,
    )
}

fn empty_manifest(module: &str) -> CapabilityManifest {
    CapabilityManifest {
        module: module.to_string(),
        requires: vec![],
    }
}

// ── RL1: Fuel limit traps an infinite-loop start function ─────────────────

// A module whose start function loops forever must trap with
// ResourceLimitExceeded when max_fuel is set to a small value.
#[test]
fn fuel_limit_traps_infinite_loop_start() {
    let wasm = wasm_infinite_loop_start();
    let manifest = empty_manifest("fuel-limit-test");
    let profile = matching_profile(
        &wasm,
        &manifest,
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: Some(100), // tiny budget — will exhaust immediately
        },
    );

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        matches!(
            result,
            Err(RuntimeError::PreflightFailed(
                PreflightFailure::ResourceLimitExceeded { .. }
            ))
        ),
        "infinite loop with tiny fuel budget must produce ResourceLimitExceeded, got {result:?}"
    );
}

// TRIANGULATE: audit log records one PreflightFailed event for the fuel trap.
#[test]
fn fuel_limit_trap_appends_audit_event() {
    let wasm = wasm_infinite_loop_start();
    let manifest = empty_manifest("fuel-audit-test");
    let profile = matching_profile(
        &wasm,
        &manifest,
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: Some(100),
        },
    );

    let mut host = RuntimeHost::new();
    let _ = host.validate_and_instantiate(&wasm, &manifest, &profile);

    let log = host.audit_log();
    assert_eq!(log.len(), 1, "exactly one audit event must be appended");
    assert!(
        !log.events()[0].is_passed(),
        "audit event must be PreflightFailed (not passed)"
    );
}

// ── RL2: Module within fuel budget succeeds ───────────────────────────────

// A module with no start function (minimal WASM header) consumes negligible
// fuel and must instantiate successfully even with a modest fuel budget.
#[test]
fn module_within_fuel_budget_instantiates() {
    let wasm = wasm_minimal();
    let manifest = empty_manifest("fuel-ok-test");
    let profile = matching_profile(
        &wasm,
        &manifest,
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: Some(10_000_000), // generous — minimal module uses near-zero fuel
        },
    );

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        result.is_ok(),
        "minimal module within fuel budget must instantiate successfully, got {result:?}"
    );
}

// ── RL3: No fuel cap (None) — module instantiates freely ─────────────────

// When max_fuel is None the host grants effectively unlimited fuel.
// A minimal module must still instantiate.
#[test]
fn no_fuel_cap_allows_instantiation() {
    let wasm = wasm_minimal();
    let manifest = empty_manifest("no-fuel-cap-test");
    let profile = matching_profile(
        &wasm,
        &manifest,
        ResourceLimits {
            max_memory_bytes: None,
            max_fuel: None,
        },
    );

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        result.is_ok(),
        "module with no fuel cap must instantiate, got {result:?}"
    );
}

// ── RL4: Memory limit blocks excessive memory.grow ────────────────────────

// A module whose start function requests 10 pages of memory (~640 KiB) must
// be denied when max_memory_bytes is set to 65536 bytes (1 page = 64 KiB).
// The module already starts with 1 page declared; growing by 10 more would
// push it to 11 pages (~704 KiB), well above the 1-page (64 KiB) limit.
#[test]
fn memory_limit_blocks_excessive_grow() {
    let wasm = wasm_memory_grow_start(10); // ask for 10 additional pages
    let manifest = empty_manifest("memory-limit-test");
    let profile = matching_profile(
        &wasm,
        &manifest,
        ResourceLimits {
            // Limit to exactly 1 WASM page (64 KiB) — less than the initial 1
            // page + 10-page grow the module requests.
            max_memory_bytes: Some(64 * 1024),
            max_fuel: None,
        },
    );

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        matches!(
            result,
            Err(RuntimeError::PreflightFailed(
                PreflightFailure::ResourceLimitExceeded { .. }
            ))
        ),
        "memory.grow beyond limit must produce ResourceLimitExceeded, got {result:?}"
    );
}

// ── RL5: Module that does not grow memory succeeds with tight cap ─────────

// A minimal module (no memory section, no start function) must instantiate
// successfully even when max_memory_bytes is set to a tiny value.
#[test]
fn memory_cap_does_not_block_module_without_memory() {
    let wasm = wasm_minimal();
    let manifest = empty_manifest("memory-ok-test");
    let profile = matching_profile(
        &wasm,
        &manifest,
        ResourceLimits {
            max_memory_bytes: Some(64 * 1024), // 64 KiB — tight but fine for a no-memory module
            max_fuel: None,
        },
    );

    let mut host = RuntimeHost::new();
    let result = host.validate_and_instantiate(&wasm, &manifest, &profile);

    assert!(
        result.is_ok(),
        "module without memory must succeed even with a tight memory cap, got {result:?}"
    );
}
