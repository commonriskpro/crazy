// ── ail-compiler::wasm_abi ────────────────────────────────────────────────
//
// WASM ABI and value-layout helpers.
//
// This module contains:
//   - `WasmScalarType` / `WasmTypeDescriptor` — structured return-type
//     descriptors used by the runtime's `invoke_typed` decoder.
//   - `derive_wasm_type` — derives a descriptor from an `AnfExpr`.
//   - `WasmSignature` — (param_count, result) pairs for module assembly.
//   - Type-inference helpers (`literal_type`, `infer_expr_type`).
//   - Binding analysis (`collect_free_vars`, `binding_params`,
//     `binding_result`, `binding_signatures`).
//   - Export name derivation (`export_name`).
//   - Record/variant layout helpers (`well_known_variant_tag`,
//     `record_layout_fields`).
//   - Effect-data layout analysis (`EffectDataLayout`, `has_effect_call`,
//     `is_structured_descriptor`, `RESULT_BUFFER_MAX`, `MAX_ARGS_BYTES`).
//
// None of these items emit WASM instructions or access `wasm_encoder` types
// beyond `ValType`.

use std::collections::BTreeMap;

use wasm_encoder::ValType;

use crate::anf::{AnfBinding, AnfExpr};
use crate::core_ir::LiteralValue;
use crate::pattern_string::arm_payload_binding;

mod bindings;
mod derive;
mod descriptors;
mod infer;
mod layout;
mod naming;

pub use derive::derive_wasm_type;
pub use descriptors::{
    ABI_VERSION, AbiDescriptor, AbiDescriptorIssue, WasmScalarType, WasmTypeDescriptor,
    WasmWireShape,
};
pub use naming::export_name;

pub(crate) use bindings::{
    binding_params, binding_result, binding_signatures, collect_free_vars, lambda_body_params,
};
pub(crate) use infer::{WasmSignature, infer_expr_type, literal_type};
pub(crate) use layout::{
    EffectDataLayout, MAX_ARGS_BYTES, RESULT_BUFFER_MAX, has_effect_call, is_structured_descriptor,
    record_layout_fields, well_known_variant_tag,
};

#[cfg(test)]
mod tests;
