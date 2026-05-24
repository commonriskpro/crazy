// ── ail-compiler::wasm_sections ──────────────────────────────────────────
//
// Pure WASM section builders — no codegen dependency.
//
// Each function assembles one WebAssembly binary section from ANF binding
// metadata and effect-layout data.  Consumed exclusively by
// `emit_wasm_with_profile` in `wasm.rs`.

use wasm_encoder::{
    ConstExpr, DataSection, EntityType, ExportKind, ExportSection, FunctionSection, GlobalSection,
    GlobalType, ImportSection, MemorySection, MemoryType, TypeSection, ValType,
};

use crate::anf::AnfBinding;
use crate::wasm_abi::{EffectDataLayout, WasmSignature, binding_result, export_name};

// ── build_type_section ────────────────────────────────────────────────────

/// Build a type section with one entry per function signature.
///
/// Returns `None` when `signatures` is empty — no type section is needed for
/// an empty module.
pub(crate) fn build_type_section(signatures: &[WasmSignature]) -> Option<TypeSection> {
    if signatures.is_empty() {
        return None;
    }
    let mut types = TypeSection::new();
    for signature in signatures {
        let params = vec![ValType::I64; signature.param_count];
        match signature.result {
            Some(result_ty) => types.ty().function(params, [result_ty]),
            None => types.ty().function(params, []),
        }
    }
    Some(types)
}

pub(crate) fn build_type_section_with_host_call(
    signatures: &[WasmSignature],
    needs_host_call_write: bool,
) -> TypeSection {
    let mut types = TypeSection::new();
    // type 0: ail/host_call — (i32 × 6) → i64
    types.ty().function(
        [
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
        ],
        [ValType::I64],
    );
    if needs_host_call_write {
        // type 1: ail/host_call_write — (i32 × 8) → i32
        types.ty().function(
            [
                ValType::I32,
                ValType::I32,
                ValType::I32,
                ValType::I32,
                ValType::I32,
                ValType::I32,
                ValType::I32,
                ValType::I32,
            ],
            [ValType::I32],
        );
    }
    for signature in signatures {
        let params = vec![ValType::I64; signature.param_count];
        match signature.result {
            Some(result_ty) => types.ty().function(params, [result_ty]),
            None => types.ty().function(params, []),
        }
    }
    types
}

// ── build_function_section ────────────────────────────────────────────────

/// Build a function section referencing type index 0 for every function.
///
/// Returns `None` when `signatures` is empty.
pub(crate) fn build_function_section(
    signatures: &[WasmSignature],
    type_offset: u32,
) -> Option<FunctionSection> {
    if signatures.is_empty() {
        return None;
    }
    let mut functions = FunctionSection::new();
    for (type_idx, _) in signatures.iter().enumerate() {
        functions.function(type_offset + type_idx as u32);
    }
    Some(functions)
}

fn build_export_section(bindings: &[AnfBinding], function_offset: u32) -> Option<ExportSection> {
    let mut exports = ExportSection::new();
    let mut count = 0usize;
    for (idx, binding) in bindings.iter().enumerate() {
        if binding_result(binding).is_some() {
            exports.export(
                &export_name(&binding.name),
                ExportKind::Func,
                function_offset + idx as u32,
            );
            count += 1;
        }
    }
    (count > 0).then_some(exports)
}

pub(crate) fn build_export_section_with_memory(
    bindings: &[AnfBinding],
    function_offset: u32,
    export_memory: bool,
) -> Option<ExportSection> {
    let mut exports = build_export_section(bindings, function_offset).unwrap_or_default();
    let mut count = usize::from(export_memory);
    if export_memory {
        exports.export("memory", ExportKind::Memory, 0);
    }
    count += bindings
        .iter()
        .filter(|binding| binding_result(binding).is_some())
        .count();
    (count > 0).then_some(exports)
}

pub(crate) fn build_import_section(
    needs_host_call: bool,
    needs_host_call_write: bool,
) -> Option<ImportSection> {
    if !needs_host_call {
        return None;
    }
    let mut imports = ImportSection::new();
    imports.import("ail", "host_call", EntityType::Function(0));
    if needs_host_call_write {
        imports.import("ail", "host_call_write", EntityType::Function(1));
    }
    Some(imports)
}

pub(crate) fn build_memory_section(needs_memory: bool) -> Option<MemorySection> {
    if !needs_memory {
        return None;
    }
    let mut memories = MemorySection::new();
    memories.memory(MemoryType {
        minimum: 1,
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });
    Some(memories)
}

pub(crate) fn build_global_section(needs_memory: bool, heap_start: i32) -> Option<GlobalSection> {
    if !needs_memory {
        return None;
    }
    let mut globals = GlobalSection::new();
    globals.global(
        GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        },
        &ConstExpr::i32_const(heap_start),
    );
    Some(globals)
}

pub(crate) fn align_to_i64(offset: i32) -> i32 {
    let offset = offset.max(8);
    ((offset + 7) / 8) * 8
}

pub(crate) fn build_data_section(layout: &EffectDataLayout) -> Option<DataSection> {
    if layout.strings.is_empty() {
        return None;
    }
    let mut data = DataSection::new();
    for (value, (ptr, _)) in &layout.strings {
        data.active(
            0,
            &ConstExpr::i32_const(*ptr),
            value.as_bytes().iter().copied(),
        );
    }
    Some(data)
}
