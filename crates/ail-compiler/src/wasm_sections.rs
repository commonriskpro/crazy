// ── ail-compiler::wasm_sections ──────────────────────────────────────────
//
// Pure WASM section builders — no codegen dependency.
//
// Each function assembles one WebAssembly binary section from ANF binding
// metadata and effect-layout data.  Consumed exclusively by
// `emit_wasm_with_profile` in `wasm.rs`.

use std::borrow::Cow;

use wasm_encoder::{
    ConstExpr, DataSection, ElementSection, Elements, EntityType, ExportKind, ExportSection,
    FunctionSection, GlobalSection, GlobalType, ImportSection, MemorySection, MemoryType, RefType,
    TableSection, TableType, TypeSection, ValType,
};

use crate::anf::AnfBinding;
use crate::wasm_abi::{EffectDataLayout, WasmSignature, binding_result, export_name};

// ── build_type_section ────────────────────────────────────────────────────

/// Build a type section with one entry per function signature.
///
/// When `needs_fold` is `true`, appends the fold-reducer type
/// `(i64, i64) → i64` at the end of the section.  Its type index is
/// `signatures.len() as u32` (the entry after all binding signatures).
///
/// Returns `None` when `signatures` is empty AND `needs_fold` is `false`.
pub(crate) fn build_type_section(
    signatures: &[WasmSignature],
    needs_fold: bool,
) -> Option<TypeSection> {
    if signatures.is_empty() && !needs_fold {
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
    if needs_fold {
        // Fold-reducer type: (i64, i64) → i64.  Appended after all binding
        // signatures so the type index = type_offset + signatures.len().
        types
            .ty()
            .function([ValType::I64, ValType::I64], [ValType::I64]);
    }
    Some(types)
}

pub(crate) fn build_type_section_with_host_call(
    signatures: &[WasmSignature],
    needs_host_call: bool,
    needs_host_call_write: bool,
    needs_resource_call: bool,
    needs_fold: bool,
) -> TypeSection {
    let mut types = TypeSection::new();
    if needs_host_call {
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
    }
    if needs_resource_call {
        // ail/resource_acquire — (res_ptr: i32, res_len: i32, args_ptr: i32, args_count: i32) → i64
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            [ValType::I64],
        );
        // ail/resource_release — (handle: i64) → ()
        types.ty().function([ValType::I64], []);
    }
    for signature in signatures {
        let params = vec![ValType::I64; signature.param_count];
        match signature.result {
            Some(result_ty) => types.ty().function(params, [result_ty]),
            None => types.ty().function(params, []),
        }
    }
    if needs_fold {
        // Fold-reducer type: (i64, i64) → i64.  Appended after host types and
        // binding signatures so the type index = type_offset + signatures.len().
        types
            .ty()
            .function([ValType::I64, ValType::I64], [ValType::I64]);
    }
    types
}

// ── build_function_section ────────────────────────────────────────────────

/// Build a function section referencing type index 0 for every function.
///
/// `hoisted_count` extra entries are appended after the binding signatures,
/// each referencing `fold_reducer_type_idx`.  These correspond to nested
/// Lambda bodies that were hoisted into the function table (Wave 12A).
///
/// Returns `None` when `signatures` is empty AND `hoisted_count == 0`.
pub(crate) fn build_function_section(
    signatures: &[WasmSignature],
    type_offset: u32,
    hoisted_count: u32,
    fold_reducer_type_idx: Option<u32>,
) -> Option<FunctionSection> {
    if signatures.is_empty() && hoisted_count == 0 {
        return None;
    }
    let mut functions = FunctionSection::new();
    for (type_idx, _) in signatures.iter().enumerate() {
        functions.function(type_offset + type_idx as u32);
    }
    // Hoisted Lambda bodies all have the fold-reducer type (i64, i64) → i64.
    if hoisted_count > 0 {
        let fold_type = fold_reducer_type_idx.unwrap_or(type_offset + signatures.len() as u32);
        for _ in 0..hoisted_count {
            functions.function(fold_type);
        }
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
    needs_resource_call: bool,
) -> Option<ImportSection> {
    if !needs_host_call && !needs_resource_call {
        return None;
    }
    let mut imports = ImportSection::new();
    let mut next_type_idx: u32 = 0;
    if needs_host_call {
        imports.import("ail", "host_call", EntityType::Function(next_type_idx));
        next_type_idx += 1;
        if needs_host_call_write {
            imports.import(
                "ail",
                "host_call_write",
                EntityType::Function(next_type_idx),
            );
            next_type_idx += 1;
        }
    }
    if needs_resource_call {
        imports.import(
            "ail",
            "resource_acquire",
            EntityType::Function(next_type_idx),
        );
        next_type_idx += 1;
        imports.import(
            "ail",
            "resource_release",
            EntityType::Function(next_type_idx),
        );
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
    if layout.strings.is_empty() && layout.bytes_entries.is_empty() {
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
    for (bytes, ptr) in &layout.bytes_entries {
        data.active(0, &ConstExpr::i32_const(*ptr), bytes.iter().copied());
    }
    Some(data)
}

// ── build_table_section ───────────────────────────────────────────────────

/// Build a table section with a single `funcref` table of `n_functions` slots.
///
/// The table holds exactly `n_functions` elements (one per compiled function
/// in the code section).  Used by `call_indirect` to dispatch Fold reducer
/// callbacks via the element section populated by `build_element_section`.
///
/// Returns `None` when `n_functions == 0` (no functions, no table needed).
pub(crate) fn build_table_section(n_functions: u32) -> Option<TableSection> {
    if n_functions == 0 {
        return None;
    }
    let mut tables = TableSection::new();
    tables.table(TableType {
        element_type: RefType::FUNCREF,
        minimum: n_functions as u64,
        maximum: Some(n_functions as u64),
        table64: false,
        shared: false,
    });
    Some(tables)
}

// ── build_element_section ─────────────────────────────────────────────────

/// Build an active element segment that populates table 0 with all compiled
/// function indices in order.
///
/// `function_offset` is the number of imported functions (host calls, resource
/// acquire/release) that precede the defined functions in the function index
/// space.  The active segment starts at table offset 0 and populates
/// `n_functions` consecutive entries:
///   `table[0] = function_offset + 0`
///   `table[1] = function_offset + 1`
///   …
///   `table[n_functions - 1] = function_offset + n_functions - 1`
///
/// This makes the table index of binding `i` equal to `i` (0-based), so
/// `call_indirect` on table index `i` dispatches to the function compiled
/// from binding `i`.
///
/// Returns `None` when `n_functions == 0`.
pub(crate) fn build_element_section(
    function_offset: u32,
    n_functions: u32,
) -> Option<ElementSection> {
    if n_functions == 0 {
        return None;
    }
    let func_indices: Vec<u32> = (function_offset..function_offset + n_functions).collect();
    let mut elements = ElementSection::new();
    // `None` table forces the MVP 0x00 encoding which implicitly references table 0.
    elements.active(
        None,
        &ConstExpr::i32_const(0),
        Elements::Functions(Cow::Owned(func_indices)),
    );
    Some(elements)
}
