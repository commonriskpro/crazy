// ── ail-compiler::native_stub ─────────────────────────────────────────────
//
// Deterministic runtime stub archive generator.
//
// Produces a valid static archive (`ail_runtime.a`) containing stub
// implementations of the three symbols imported by native objects:
//
//   host_call(i64 × 6) → i64           — capability dispatch no-op; returns -1
//   __ail_malloc(i64)  → i64           — allocator stub; returns 0 (null; smoke-test only)
//   ail_runtime_call(i64 × 3) → i64   — runtime dispatch no-op; returns -1
//
// # Design
//
// Uses Cranelift (the same engine used by `emit_native`) to emit a
// platform-native object file containing the three stub functions, then
// wraps it in a minimal BSD/GNU `ar` archive in pure Rust — no system
// `ar`, `cc`, or linker is required.
//
// The generated archive can be passed directly to the system linker:
//   cc prog.o ail_runtime.a -o prog
// or via `ail link --runtime-lib ail_runtime.a`.
//
// # Determinism
//
// `build_runtime_stub_archive()` is deterministic: the same bytes are
// produced on every call on the same host ISA.  Timestamps in the `ar`
// header are zeroed (epoch) for reproducibility.
//
// # Calling-convention parity
//
// Uses `CallConv::SystemV` — the same convention declared in `native.rs`
// for the matching imported symbols.  The stubs pair correctly with any
// native object emitted by `emit_native`.
//
// # CI safety
//
// No platform-specific tools are invoked.  Tests that verify the archive
// format are pure-Rust and work on any host.

use cranelift_codegen::{
    Context,
    ir::{AbiParam, Function, InstBuilder, Signature, UserFuncName, types},
    isa::CallConv,
    settings,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};

use crate::error::CompileError;

// ── Public constants ──────────────────────────────────────────────────────

/// Runtime symbols potentially imported by native objects emitted by
/// `emit_native`, conditional on the `needs_host_call`, `needs_heap_alloc`,
/// and `needs_runtime_call` flags computed from the data layout.  A given
/// native object may import only a subset of these.  Listed in definition
/// order (matches `native.rs`).
///
/// Used for diagnostic output and to drive stub generation.
pub const RUNTIME_SYMBOLS: [&str; 3] = ["host_call", "__ail_malloc", "ail_runtime_call"];

// ── Public API ────────────────────────────────────────────────────────────

/// Emit a platform-native object file containing stub implementations of all
/// three runtime symbols ([`RUNTIME_SYMBOLS`]).
///
/// The returned bytes are a valid ELF / Mach-O / COFF object file for the
/// host ISA.  They can be archived with [`build_runtime_stub_archive`] or
/// inspected directly.
///
/// # Errors
///
/// Returns [`CompileError::NativeEncodingError`] if Cranelift fails to
/// compile or emit the object (e.g. unsupported host ISA).
pub fn build_runtime_stub_object() -> Result<Vec<u8>, CompileError> {
    let flags = settings::Flags::new(settings::builder());
    let isa = cranelift_native::builder()
        .map_err(|e| CompileError::NativeEncodingError(format!("stub ISA builder: {e}")))?
        .finish(flags)
        .map_err(|e| CompileError::NativeEncodingError(format!("stub ISA finish: {e}")))?;

    let obj_builder = ObjectBuilder::new(
        isa,
        "ail_runtime_stub",
        cranelift_module::default_libcall_names(),
    )
    .map_err(|e| CompileError::NativeEncodingError(format!("stub ObjectBuilder: {e}")))?;
    let mut module = ObjectModule::new(obj_builder);

    // host_call(i64 × 6) → i64  — returns -1 (no-op denial)
    define_stub(&mut module, "host_call", 6, -1)?;

    // __ail_malloc(i64) → i64   — returns 0 (null pointer; smoke-test stub only;
    //                             any code that dereferences the result will trap
    //                             or segfault — do not use in production binaries)
    define_stub(&mut module, "__ail_malloc", 1, 0)?;

    // ail_runtime_call(i64 × 3) → i64 — returns -1 (no-op denial)
    define_stub(&mut module, "ail_runtime_call", 3, -1)?;

    let product = module.finish();
    product
        .emit()
        .map_err(|e| CompileError::NativeEncodingError(format!("stub object emit: {e}")))
}

/// Build a deterministic static archive (`ail_runtime.a`) containing the
/// runtime stubs.
///
/// The archive is in BSD/GNU `ar` format and can be passed directly to any
/// system linker (`cc`, `clang`, `lld`) to resolve the unresolved imports
/// in a native object emitted by `emit_native`.
///
/// # Errors
///
/// Propagates errors from [`build_runtime_stub_object`].
pub fn build_runtime_stub_archive() -> Result<Vec<u8>, CompileError> {
    let object_bytes = build_runtime_stub_object()?;
    Ok(wrap_in_ar_archive(&object_bytes))
}

// ── ar archive writer ─────────────────────────────────────────────────────

/// Wrap `object_bytes` in a minimal BSD/GNU `ar` archive.
///
/// The archive contains a single member named `stub.o`.  Timestamps, UID,
/// and GID are zeroed for determinism.
///
/// Layout:
/// ```text
/// "!<arch>\n"                 —  8 bytes: global header
/// <member header: 60 bytes>
/// <object bytes>
/// [\n]                        —  optional 1-byte padding to even boundary
/// ```
///
/// Member header fields (each space-padded to the declared width):
/// ```text
/// name[16]    "stub.o          "
/// mtime[12]   "0           "
/// uid[6]      "0     "
/// gid[6]      "0     "
/// mode[8]     "0644    "
/// size[10]    decimal byte count of object_bytes
/// end[2]      "`\n"
/// ```
fn wrap_in_ar_archive(object_bytes: &[u8]) -> Vec<u8> {
    let size = object_bytes.len();
    let size_str = format!("{size:<10}"); // left-align decimal, space-padded to 10 chars

    let mut out = Vec::with_capacity(8 + 60 + size + 1);

    // Global header (8 bytes)
    out.extend_from_slice(b"!<arch>\n");

    // Member header (60 bytes total)
    out.extend_from_slice(b"stub.o          "); // name:  16 bytes
    out.extend_from_slice(b"0           "); //     mtime: 12 bytes
    out.extend_from_slice(b"0     "); //           uid:   6 bytes
    out.extend_from_slice(b"0     "); //           gid:   6 bytes
    out.extend_from_slice(b"0644    "); //          mode:  8 bytes
    out.extend_from_slice(size_str.as_bytes()); //  size:  10 bytes
    out.extend_from_slice(b"`\n"); //               end:   2 bytes

    // Object data
    out.extend_from_slice(object_bytes);

    // Even-boundary padding
    if !size.is_multiple_of(2) {
        out.push(b'\n');
    }

    out
}

// ── Cranelift stub function builder ──────────────────────────────────────

/// Define and emit a single-block stub function in `module`.
///
/// The stub takes `param_count` I64 parameters (all ignored) and returns
/// `return_val` as an I64 constant.  Exported with `Linkage::Export`.
fn define_stub(
    module: &mut ObjectModule,
    name: &str,
    param_count: usize,
    return_val: i64,
) -> Result<(), CompileError> {
    let mut sig = Signature::new(CallConv::SystemV);
    for _ in 0..param_count {
        sig.params.push(AbiParam::new(types::I64));
    }
    sig.returns.push(AbiParam::new(types::I64));

    let func_id = module
        .declare_function(name, Linkage::Export, &sig)
        .map_err(|e| {
            CompileError::NativeEncodingError(format!("stub declare_function({name}): {e}"))
        })?;

    let mut func = Function::with_name_signature(UserFuncName::user(0, func_id.as_u32()), sig);
    let mut fn_ctx = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut func, &mut fn_ctx);

    let block = builder.create_block();
    builder.append_block_params_for_function_params(block);
    builder.switch_to_block(block);
    builder.seal_block(block);

    let ret = builder.ins().iconst(types::I64, return_val);
    builder.ins().return_(&[ret]);
    builder.finalize();

    let mut ctx = Context::for_function(func);
    module.define_function(func_id, &mut ctx).map_err(|e| {
        CompileError::NativeEncodingError(format!("stub define_function({name}): {e}"))
    })?;

    Ok(())
}
