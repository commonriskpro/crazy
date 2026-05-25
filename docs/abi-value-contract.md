# WASM ABI and value memory contract

This document locks the current contract between `ail-compiler` and
`ail-runtime`. It describes the implemented Wave 3A subset only, not the broader
target language design.

The compiler owns layout emission. The runtime owns defensive decoding from a
caller-provided `ValueLayout`. The descriptor metadata is carried in
`WasmArtifact::export_types`; it is not embedded in the `.wasm` binary.

## Scalar returns

| Source value | WASM return | Runtime value |
| --- | --- | --- |
| `Int` / `Bool` | `i64` | `StructuredValue::Scalar(i64)`; bool uses `0` or `1` |
| `Text` | `i64` | `StructuredValue::Text { ptr, len }` decoded from packed `(len << 32) \| ptr` via `ValueLayout::Text` |
| `Float` | `f64` | `StructuredValue::Float(f64)` from `invoke_typed` |
| `Unit` | `i32` value `0` | `StructuredValue::Scalar(0)` when decoded as `Scalar`; raw no-result exports become `Unit` |
| `Handle` | numeric id | `StructuredValue::Handle(HandleId)` |

## Structured memory layouts

All structured returns are `i32` pointers into exported WASM linear memory. The
typed runtime widens that pointer to `i64` before decoding. Pointers are valid
only for the current invocation and while the `RuntimeInstance` remains alive;
the guest allocator is a monotonic bump pointer and the host does not take
ownership of guest memory.

| Layout | Memory at returned pointer |
| --- | --- |
| Record | Field slots are contiguous 8-byte little-endian `i64` values in declaration order: field `0` at `ptr + 0`, field `1` at `ptr + 8`, etc. |
| Tuple | Element slots are contiguous 8-byte little-endian values in tuple order with no count prefix. Runtime currently decodes tuples into `StructuredValue::List`. |
| List | `i64` element count at `ptr + 0`, followed by contiguous 8-byte element slots starting at `ptr + 8`. |
| Variant | `i32` tag at `ptr + 0`, 4 bytes padding/reserved at `ptr + 4`, payload slot at `ptr + 8`. Payloads are one 8-byte ABI slot. |
| Option | Variant layout with stable tags `None = 0`, `Some = 1`. `None` has no meaningful payload. |
| Result | Variant layout with stable tags `Ok = 0`, `Err = 1`. |

Nested structured values are represented by storing the nested value pointer in
an 8-byte slot. Scalar slots use little-endian `i64`; `i32` values are widened
before storage when needed.

## Failure behavior

Runtime decoding is fail-closed for impossible structured layouts:

| Condition | Behavior |
| --- | --- |
| Missing exported memory for a pointer layout | Decode with an empty memory slice and return `StructuredValue::Unit`. |
| Negative or non-`i32` pointer | Return `StructuredValue::Unit`. |
| Out-of-bounds tag, count, field, element, or payload read | Return `StructuredValue::Unit`. |
| Unknown variant tag index | Return `StructuredValue::Unit`. |
| Unsupported result value type from WASM | `RuntimeError::EncodingError` from `invoke`. |
| Unsupported descriptor/layout not represented by `ValueLayout` | Not part of the implemented ABI contract. |

## ABI versioning

`ail-compiler` exposes `ABI_VERSION: u32 = 1` and an `AbiDescriptor` struct that
wraps `export_types` with the current version.  `WasmArtifact::abi_descriptor`
is populated by `emit_wasm` and may be serialised (JSON/CBOR) and passed across a
process boundary.  The runtime caller checks `AbiDescriptor::is_compatible()`
before invoking typed exports.  When the typed-value layout contract changes in a
backward-incompatible way, `ABI_VERSION` must be incremented.

The `HandleRegistry` tracks handles by reference count, not by a simple active/
inactive flag.  `create()` starts at count 1; `clone_handle()` increments the
count (for Shared-mode handles); `release()` decrements the count and returns
`true` only when the count reaches zero (full release).

## Compiler side

`ail-compiler::wasm::WasmArtifact::export_types` records one
`WasmTypeDescriptor` per exported function. The descriptor is metadata for the
runtime caller; it is not embedded into the WASM module.

The current descriptor contract recognized by the compiler/runtime boundary is:

> This table describes the WASM runtime boundary. The native backend can emit
> the same packed scalar representation in object data, but it does not use the
> `ail-runtime` decode path.

| Compiler descriptor | Runtime layout | Current payload shape |
| --- | --- | --- |
| `Scalar(I64)` | `ValueLayout::Scalar` | raw `i64` return |
| `Scalar(I32)` | `ValueLayout::Scalar` | raw `i32` return widened by the typed runtime entry point |
| `Scalar(F64)` | `ValueLayout::Scalar` | raw `f64`; `invoke_typed` returns `StructuredValue::Float` directly |
| `Text` | `ValueLayout::Text` | packed `(len << 32) \| ptr` i64; runtime unpacks to `StructuredValue::Text { ptr, len }` without a memory read |
| `Bytes` | `ValueLayout::Bytes` | packed `(len << 32) \| ptr` i64; runtime unpacks to `StructuredValue::Bytes { ptr, len }` without a memory read; no UTF-8 assumption |
| `Record { fields }` | `ValueLayout::Record { fields }` | pointer to sequential `i64` field slots in linear memory |
| `Variant { tags }` | `ValueLayout::Variant { tags }` | pointer to `i32` tag at offset `0`, optional `i64` payload at offset `8` |
| `Tuple(elems)` | `ValueLayout::Tuple(elems)` | pointer to sequential `i64` element slots in linear memory |
| `List(inner)` | `ValueLayout::List(inner)` | pointer to `i64` count followed by `i64` element slots |
| `Option(inner)` | `ValueLayout::Option(inner)` | variant layout with tags `None` and `Some` |
| `Result { ok, err }` | `ValueLayout::Result { ok, err }` | variant layout with tags `Ok` and `Err` |
| `Handle` | `ValueLayout::Handle` | raw numeric handle id |

`derive_wasm_type()` currently derives only part of that table from ANF on its
own: scalar literals, records, variants, tuples, lists, and let bodies. `Option`,
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

- `Bytes` literals (`LiteralValue::Bytes(Vec<u8>)`) are now executable in both
  the WASM and native backends.  In WASM the compiler interns the byte buffer in
  the data section and emits a packed `(len << 32) | ptr` i64 return — the same
  encoding used for `Text`.  In the native backend the same packed encoding is
  used: the byte buffer is placed in a `__ail_bytes_N` local data object and the
  Cranelift IR emits `symbol_value + ishl_imm(32) + bor`.  (WASM only) The
  runtime decodes this via `ValueLayout::Bytes` →
  `StructuredValue::Bytes { ptr, len }`; native emits the same packed i64 in
  object data but has no ail-runtime decode path.
  `derive_wasm_type` maps `Literal(Bytes(_))` to `WasmTypeDescriptor::Bytes` so
  callers receive the correct descriptor.  `infer_cranelift_return_type` returns
  `I64` for `Bytes` on the native path.
- `Unit` is currently represented by the compiler as `Scalar(I32)`. The runtime
  decoder maps `ValueLayout::Scalar` to `StructuredValue::Scalar(raw)`; only a
  raw `RuntimeValue::Unit` from `invoke` becomes `StructuredValue::Unit` in
  `invoke_typed`.
- Records and generic variants currently decode scalar payload slots. Tuples,
  lists, options, and results can carry nested `ValueLayout` metadata, but broad
  RC/GC/object-model semantics are out of scope for this subset.
- `derive_wasm_type` derives `Option`, `Result`, and `Handle` descriptors only
  in specific ANF shapes. Broad compiler emission coverage for these across all
  expression forms is a future executable-surface milestone.
