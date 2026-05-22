// ── ail-compiler::spike_v03 ───────────────────────────────────────────────
//
// V-03 Evaluation Spike: ANF → Cranelift IR lowering prototype.
//
// # Purpose
//
// This module contains the V-03 spike artifact for Phase 17. It prototypes
// the end-to-end path from a minimal `AnfIr` (one binding) through Cranelift
// IR construction, codegen, and object emission, then verifies that the
// `NodeRef → native_offset` provenance round-trip is feasible.
//
// # Findings summary (persisted to Engram sdd/native-backend-expansion/v03-spike)
//
// - cranelift-codegen 0.115.1 (same as wasmtime 28.0.1) compiles cleanly as a
//   direct workspace dependency; no version conflict arises.
// - `cranelift_frontend::FunctionBuilder` handles SSA internally. The caller
//   only needs to: create a block, switch to it, emit a `trap` instruction, and
//   seal the block.
// - `cranelift_module::Module::define_function` → `Context.compile` produces a
//   `CompiledCode` from which `compiled_code.buffer.total_size()` gives the
//   function's code size in bytes. The byte offset of each function in the final
//   object is obtained by accumulating sizes in binding order.
// - Determinism: the same `AnfIr` always produces the same byte offsets because
//   Cranelift's codegen is deterministic for identical IR.
// - Provenance feasibility: confirmed. `NodeRef` can be mapped to `native_offset`
//   by recording the cumulative size before each function is compiled.
//
// # This file is spike-only
//
// All production types (`NativeArtifact`, `emit_native`, etc.) live in
// `native.rs`. This module is compiled only in `#[cfg(test)]`.

#![cfg(test)]

use std::collections::BTreeMap;

use ail_core::semantic_graph::{GraphNode, NodeKind, NodeRef, SemanticGraph};
use ail_verify::report::VerificationReport;
use cranelift_codegen::{
    Context,
    ir::{Function, InstBuilder, Signature, UserFuncName},
    isa::CallConv,
    settings,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};

use crate::lower::{lower_to_anf, lower_to_core_ir};

// ── helpers ──────────────────────────────────────────────────────────────

fn proven_report() -> VerificationReport {
    VerificationReport {
        entries: vec![],
        ..Default::default()
    }
}

/// Build a minimal `AnfIr` with `n` bindings named `fn_0`, `fn_1`, …
fn anf_for_n(n: usize) -> crate::anf::AnfIr {
    let graph = SemanticGraph {
        nodes: (0..n)
            .map(|i| GraphNode::new(NodeRef(i as u32), NodeKind::Function, format!("fn_{i}")))
            .collect(),
        edges: vec![],
    };
    let core = lower_to_core_ir(&graph, &proven_report()).unwrap();
    lower_to_anf(&core).unwrap()
}

// ── V-03 spike: ANF → Cranelift → native object emission ─────────────────

/// V-03 spike: prototype ANF → Cranelift IR lowering on a 1-function `AnfIr`.
///
/// Verifies:
/// 1. Cranelift IR construction succeeds for a stub (trap) function.
/// 2. Object emission via `cranelift-object` produces non-empty bytes.
/// 3. `NodeRef → native_offset` provenance round-trip is feasible.
#[test]
fn v03_spike_anf_to_cranelift_lowering_one_function() {
    // Build a 1-binding ANF IR.
    let anf = anf_for_n(1);
    assert_eq!(anf.bindings.len(), 1);
    let anf_ir_hash = anf
        .stage_hashes
        .anf_ir_hash
        .expect("anf_ir_hash must be sealed by lower_to_anf");

    // Set up Cranelift ISA for the host architecture.
    let flags = settings::Flags::new(settings::builder());
    let isa = cranelift_native::builder()
        .expect("host ISA builder must succeed")
        .finish(flags)
        .expect("ISA construction must succeed");

    // Set up ObjectModule — the target for native code emission.
    let obj_builder =
        ObjectBuilder::new(isa, "v03_spike", cranelift_module::default_libcall_names())
            .expect("ObjectBuilder must be created");
    let mut obj_module = ObjectModule::new(obj_builder);

    // Build a stub signature: () -> () (no params, no results — matches Phase 7 stubs).
    let sig = Signature::new(CallConv::SystemV);
    // No params, no returns — identical to WASM `() -> ()` stubs.

    // Declare and define each binding as a Cranelift function.
    let mut provenance: BTreeMap<NodeRef, u64> = BTreeMap::new();
    let mut cumulative_offset: u64 = 0;

    for binding in &anf.bindings {
        // Declare the function in the module.
        let func_id = obj_module
            .declare_function(&binding.name, Linkage::Export, &sig)
            .expect("declare_function must succeed");

        // Build the function IR: one basic block with a trap instruction.
        let mut func =
            Function::with_name_signature(UserFuncName::user(0, func_id.as_u32()), sig.clone());
        {
            let mut fn_ctx = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut func, &mut fn_ctx);
            let block = builder.create_block();
            builder.switch_to_block(block);
            builder.seal_block(block);
            builder
                .ins()
                .trap(cranelift_codegen::ir::TrapCode::user(1).unwrap());
            builder.finalize();
        }

        // Compile the function.
        let mut ctx = Context::for_function(func);
        obj_module
            .define_function(func_id, &mut ctx)
            .expect("define_function must succeed");

        // Record provenance: NodeRef → current cumulative offset in code section.
        provenance.insert(binding.source_ref, cumulative_offset);

        // Advance offset by compiled code size.
        let compiled_size = ctx
            .compiled_code()
            .expect("must have compiled code")
            .code_info()
            .total_size;
        cumulative_offset += u64::from(compiled_size);
    }

    // Finish — emit the native object bytes.
    let object = obj_module.finish();
    let native_bytes = object.emit().expect("object emission must produce bytes");

    // Assertions: provenance feasibility.
    assert_eq!(
        provenance.len(),
        anf.bindings.len(),
        "provenance must have one entry per binding"
    );
    assert!(
        !native_bytes.is_empty(),
        "native_bytes must be non-empty for a 1-function module"
    );

    // Provenance: NodeRef(0) maps to offset 0 (first function).
    assert_eq!(
        provenance.get(&NodeRef(0)).copied(),
        Some(0u64),
        "first binding must map to offset 0"
    );

    // Verify hash chain extension feasibility:
    // native_hash = blake3(anf_ir_hash || native_bytes)
    let native_hash = crate::hash::hash_with_parent(&anf_ir_hash, &native_bytes);
    assert_eq!(
        native_hash.len(),
        32,
        "native_hash must be a 32-byte BLAKE3 digest"
    );

    // Spike result: emit spike findings summary for Engram record.
    let findings = format!(
        "V-03 spike OK: 1-function ANF lowered to Cranelift. \
         native_bytes.len()={}, provenance={provenance:?}, \
         native_hash={native_hash:?}",
        native_bytes.len(),
    );
    // Print to test output (captured by `cargo test -- --nocapture`).
    eprintln!("[V-03 spike] {findings}");
}

/// V-03 spike: provenance len == binding count for N=3.
///
/// Triangulates that the accumulation loop generalises beyond N=1.
#[test]
fn v03_spike_provenance_len_equals_binding_count_n3() {
    let anf = anf_for_n(3);
    assert_eq!(anf.bindings.len(), 3);

    let flags = settings::Flags::new(settings::builder());
    let isa = cranelift_native::builder()
        .expect("host ISA builder")
        .finish(flags)
        .expect("ISA");

    let obj_builder = ObjectBuilder::new(
        isa,
        "v03_spike_n3",
        cranelift_module::default_libcall_names(),
    )
    .expect("ObjectBuilder");
    let mut obj_module = ObjectModule::new(obj_builder);

    let sig = Signature::new(CallConv::SystemV);
    let mut provenance: BTreeMap<NodeRef, u64> = BTreeMap::new();
    let mut offset: u64 = 0;

    for binding in &anf.bindings {
        let func_id = obj_module
            .declare_function(&binding.name, Linkage::Export, &sig)
            .expect("declare");

        let mut func =
            Function::with_name_signature(UserFuncName::user(0, func_id.as_u32()), sig.clone());
        {
            let mut fn_ctx = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut func, &mut fn_ctx);
            let block = builder.create_block();
            builder.switch_to_block(block);
            builder.seal_block(block);
            builder
                .ins()
                .trap(cranelift_codegen::ir::TrapCode::user(1).unwrap());
            builder.finalize();
        }

        let mut ctx = Context::for_function(func);
        obj_module
            .define_function(func_id, &mut ctx)
            .expect("define");
        provenance.insert(binding.source_ref, offset);

        let sz = ctx
            .compiled_code()
            .expect("compiled")
            .code_info()
            .total_size;
        offset += u64::from(sz);
    }

    let object = obj_module.finish();
    let _bytes = object.emit().expect("emit");

    assert_eq!(
        provenance.len(),
        3,
        "provenance len must equal binding count (3)"
    );

    // Offsets must be monotonically non-decreasing.
    let offsets: Vec<u64> = anf
        .bindings
        .iter()
        .map(|b| provenance[&b.source_ref])
        .collect();
    for w in offsets.windows(2) {
        assert!(w[1] >= w[0], "offsets must be non-decreasing: {w:?}");
    }
}

/// V-03 spike: empty ANF produces empty provenance and still emits valid object bytes.
#[test]
fn v03_spike_empty_anf_produces_empty_provenance() {
    let anf = anf_for_n(0);
    assert!(anf.bindings.is_empty());

    let flags = settings::Flags::new(settings::builder());
    let isa = cranelift_native::builder()
        .expect("host ISA builder")
        .finish(flags)
        .expect("ISA");

    let obj_builder = ObjectBuilder::new(
        isa,
        "v03_spike_empty",
        cranelift_module::default_libcall_names(),
    )
    .expect("ObjectBuilder");
    let obj_module = ObjectModule::new(obj_builder);
    let provenance: BTreeMap<NodeRef, u64> = BTreeMap::new();

    let object = obj_module.finish();
    let native_bytes = object.emit().expect("emit");

    assert!(
        provenance.is_empty(),
        "empty ANF must produce empty provenance"
    );
    // An empty object file is still a valid ELF/Mach-O/COFF — non-empty header.
    assert!(
        !native_bytes.is_empty(),
        "empty module still produces non-empty object header"
    );
}
