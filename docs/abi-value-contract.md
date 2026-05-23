# ABI/value contract: implemented subset

This document locks the current ABI/value contract between `ail-compiler` and
`ail-runtime`. It describes what is implemented today, not the broader target
language design.

## Compiler side

`ail-compiler::wasm::WasmArtifact::export_types` records one
`WasmTypeDescriptor` per exported function. The descriptor is metadata for the
runtime caller; it is not embedded into the WASM module.

The current descriptor contract recognized by the compiler/runtime boundary is:

| Compiler descriptor | Runtime layout | Current payload shape |
| --- | --- | --- |
| `Scalar(I64)` | `ValueLayout::Scalar` | raw `i64` return |
| `Scalar(I32)` | `ValueLayout::Scalar` | raw `i32` return widened by the typed runtime entry point |
| `Scalar(F64)` | `ValueLayout::Scalar` | raw `f64`; `invoke_typed` returns `StructuredValue::Float` directly |
| `Record { fields }` | `ValueLayout::Record { fields }` | pointer to sequential `i64` field slots in linear memory |
| `Variant { tags }` | `ValueLayout::Variant { tags }` | pointer to `i32` tag at offset `0`, optional `i64` payload at offset `8` |
| `List(inner)` | `ValueLayout::List(inner)` | pointer to `i64` count followed by `i64` element slots |
| `Option(inner)` | `ValueLayout::Option(inner)` | variant layout with tags `None` and `Some` |
| `Result { ok, err }` | `ValueLayout::Result { ok, err }` | variant layout with tags `Ok` and `Err` |
| `Handle` | `ValueLayout::Handle` | raw numeric handle id |

`derive_wasm_type()` currently derives only part of that table from ANF on its
own: scalar literals, records, variants, lists/tuples, and let bodies. `Option`,
`Result`, and `Handle` descriptors are represented in the contract and can be
decoded by the runtime, but broad compiler emission coverage for them is still a
future executable-surface milestone.

## Runtime side

`ail-runtime::codec::ValueLayout` is the runtime mirror of
`WasmTypeDescriptor`. `RuntimeInstance::invoke_typed` accepts a `ValueLayout`
provided by the caller, invokes the export, reads WASM memory when needed, and
delegates decoding to `ValueDecoder`.

The runtime crate intentionally does not depend on `ail-compiler`; callers that
own a `WasmArtifact` are responsible for translating `export_types` into
`ValueLayout` before invoking typed exports.

## Current limitations

- `Text` is currently represented by the compiler as `Scalar(I64)` containing a
  packed `(ptr, len)` value. The runtime has `StructuredValue::Text`, but there
  is no `ValueLayout::Text` in the implemented descriptor contract yet.
- `Bytes` exists in the target/core type design, but there is no ANF literal or
  `WasmTypeDescriptor::Bytes` in the implemented ABI contract.
- `Unit` is currently represented by the compiler as `Scalar(I32)`. The runtime
  decoder maps `ValueLayout::Scalar` to `StructuredValue::Scalar(raw)`; only a
  raw `RuntimeValue::Unit` from `invoke` becomes `StructuredValue::Unit` in
  `invoke_typed`.
- Records, variants, lists, options, and results currently decode scalar payload
  slots only. Nested descriptors are represented, but broad RC/GC/object-model
  semantics are out of scope for this subset.
