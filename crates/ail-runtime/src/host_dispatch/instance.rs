// ── ail-runtime::host_dispatch::instance ──────────────────────────────────

use std::fmt;

use wasmtime::{Extern, Module, Store, Val};

use crate::audit::AuditLog;
use crate::codec::{StructuredValue, ValueDecoder, ValueLayout};
use crate::error::{RuntimeError, RuntimeResult};
use crate::host_dispatch::diagnostics::{
    WasmBridgeDiagnostic, WasmBridgeInvokeError, diagnose_func_abi, sort_wasm_bridge_diagnostics,
};
use crate::host_dispatch::result_diagnostics::HostDispatchResultDiagnostic;
use crate::host_dispatch::state::HostState;
use crate::host_dispatch::trace::TraceContext;
use crate::host_dispatch::values::{RuntimeArg, RuntimeValue, runtime_arg_to_val};

// ── RuntimeInstance ───────────────────────────────────────────────────────

/// A validated and instantiated WASM module ready for future execution.
///
/// Uses `Store<HostState>` so that Wasmtime host-function closures can
/// dispatch capability calls to registered handlers.
pub struct RuntimeInstance {
    // Keep the module alive for future metadata/call APIs.
    pub(super) _module: Module,
    pub(super) store: Store<HostState>,
    pub(super) instance: wasmtime::Instance,
}

impl RuntimeInstance {
    /// Count exports visible from the instantiated module.
    ///
    /// Current compiler output has no exports, but this method proves tests are
    /// observing a real Wasmtime `Instance` rather than only a compiled module.
    pub fn export_count(&mut self) -> usize {
        self.instance.exports(&mut self.store).count()
    }

    /// Return stable redacted diagnostics for invoking `export_name` with `args`.
    ///
    /// This is a non-executing ABI check: it reports missing exports and
    /// export signature mismatches without calling into WASM.  Use
    /// [`invoke_with_bridge_diagnostics`](Self::invoke_with_bridge_diagnostics)
    /// when trap classification is also required.
    pub fn wasm_bridge_diagnostics_for_call(
        &mut self,
        export_name: &str,
        args: &[RuntimeArg],
    ) -> Vec<WasmBridgeDiagnostic> {
        let export = self.instance.get_export(&mut self.store, export_name);
        let diagnostics = match export {
            None => vec![WasmBridgeDiagnostic::missing_export(export_name)],
            Some(Extern::Func(func)) => {
                let func_ty = func.ty(&self.store);
                let params = func_ty.params().collect::<Vec<_>>();
                let results = func_ty.results().collect::<Vec<_>>();
                diagnose_func_abi(export_name, &params, &results, args)
            }
            Some(other) => vec![WasmBridgeDiagnostic::export_abi_mismatch(
                export_name,
                "export.kind",
                format!("expected func; actual {}", extern_kind(&other)),
            )],
        };
        sort_wasm_bridge_diagnostics(diagnostics)
    }

    /// Invoke an export and attach stable redacted WASM bridge diagnostics on failure.
    ///
    /// Existing [`invoke`](Self::invoke) behavior is preserved.  This method is
    /// additive for production runtimes that need deterministic missing export,
    /// ABI mismatch, and trap classification without exposing raw labels.
    pub fn invoke_with_bridge_diagnostics(
        &mut self,
        export_name: &str,
        args: &[RuntimeArg],
    ) -> Result<RuntimeValue, WasmBridgeInvokeError> {
        let diagnostics = self.wasm_bridge_diagnostics_for_call(export_name, args);
        if !diagnostics.is_empty() {
            let source = self
                .invoke(export_name, args)
                .unwrap_err_or_encoding_error(export_name);
            return Err(WasmBridgeInvokeError {
                source,
                diagnostics,
            });
        }

        self.invoke(export_name, args).map_err(|source| {
            let diagnostics = vec![WasmBridgeDiagnostic::trap(export_name, &source)];
            WasmBridgeInvokeError {
                source,
                diagnostics,
            }
        })
    }

    pub fn invoke(
        &mut self,
        export_name: &str,
        args: &[RuntimeArg],
    ) -> RuntimeResult<RuntimeValue> {
        let func = self
            .instance
            .get_func(&mut self.store, export_name)
            .ok_or_else(|| {
                RuntimeError::EncodingError(format!("export `{export_name}` not found"))
            })?;
        let func_ty = func.ty(&self.store);
        let params = func_ty.params().collect::<Vec<_>>();
        if params.len() != args.len() {
            return Err(RuntimeError::EncodingError(format!(
                "export `{export_name}` expects {} args, got {}",
                params.len(),
                args.len()
            )));
        }
        if params.iter().any(|ty| {
            !matches!(
                ty,
                wasmtime::ValType::I64 | wasmtime::ValType::I32 | wasmtime::ValType::F64
            )
        }) {
            return Err(RuntimeError::EncodingError(format!(
                "export `{export_name}` only supports i64/i32/f64 parameters"
            )));
        }
        let results = func_ty.results().collect::<Vec<_>>();
        if results.len() > 1 {
            return Err(RuntimeError::EncodingError(format!(
                "export `{export_name}` returned {} values; at most one is supported",
                results.len()
            )));
        }
        if results.iter().any(|ty| {
            !matches!(
                ty,
                wasmtime::ValType::I64 | wasmtime::ValType::I32 | wasmtime::ValType::F64
            )
        }) {
            return Err(RuntimeError::EncodingError(format!(
                "export `{export_name}` only supports i64/i32/f64 results"
            )));
        }

        let wasm_args = args.iter().map(runtime_arg_to_val).collect::<Vec<_>>();
        let mut wasm_results = results
            .iter()
            .map(|ty| {
                Val::default_for_ty(ty).ok_or_else(|| {
                    RuntimeError::EncodingError(format!(
                        "export `{export_name}` has unsupported result type {ty}"
                    ))
                })
            })
            .collect::<RuntimeResult<Vec<_>>>()?;

        self.store.data_mut().capability_calls_used = 0;
        func.call(&mut self.store, &wasm_args, &mut wasm_results)
            .map_err(|e| RuntimeError::EncodingError(format!("export `{export_name}`: {e}")))?;

        match wasm_results.as_slice() {
            [] => Ok(RuntimeValue::Unit),
            [Val::I64(value)] => Ok(RuntimeValue::I64(*value)),
            [Val::I32(value)] => Ok(RuntimeValue::I32(*value)),
            [Val::F64(bits)] => Ok(RuntimeValue::F64(f64::from_bits(*bits))),
            [value] => Err(RuntimeError::EncodingError(format!(
                "export `{export_name}` returned unsupported value {value:?}"
            ))),
            _ => unreachable!("multiple results rejected before invocation"),
        }
    }

    /// Return sorted, deduplicated redacted diagnostics recorded by WASM-side host dispatch.
    pub fn host_dispatch_result_diagnostics(&self) -> Vec<HostDispatchResultDiagnostic> {
        self.store.data().dispatch_result_diagnostics()
    }

    /// Clear recorded WASM-side host dispatch result diagnostics.
    pub fn clear_host_dispatch_result_diagnostics(&mut self) {
        self.store.data_mut().clear_dispatch_result_diagnostics();
    }

    /// Read `len` bytes from WASM linear memory starting at `ptr`.
    ///
    /// Returns `None` if:
    /// - `ptr` is negative.
    /// - The module has no exported `"memory"`.
    /// - The read range `[ptr, ptr + len)` exceeds the memory size.
    pub fn read_wasm_memory(&mut self, ptr: i32, len: usize) -> Option<Vec<u8>> {
        if ptr < 0 {
            return None;
        }
        let memory = self.instance.get_memory(&mut self.store, "memory")?;
        let mut buf = vec![0u8; len];
        memory.read(&self.store, ptr as usize, &mut buf).ok()?;
        Some(buf)
    }

    /// Read a little-endian `i64` from WASM linear memory at `ptr + byte_offset`.
    ///
    /// This is a **read-only, bounds-checked** helper intended for test-side
    /// memory introspection.  It does not modify the WASM linear memory.
    ///
    /// Returns `None` if:
    /// - Either `ptr` or `byte_offset` is negative.
    /// - `ptr + byte_offset` overflows a `u32` (WASM memory is 32-bit addressed).
    /// - `ptr + byte_offset` exceeds `i32::MAX` (would become negative when passed
    ///   to the memory accessor).
    /// - The module has no exported `"memory"`.
    /// - The 8-byte read range `[ptr + byte_offset, ptr + byte_offset + 8)` is
    ///   out of bounds for the current linear-memory size.
    pub fn read_memory_i64(&mut self, ptr: i32, byte_offset: i32) -> Option<i64> {
        if ptr < 0 || byte_offset < 0 {
            return None;
        }
        // Use u32 arithmetic to detect overflow before narrowing to i32.
        let base = (ptr as u32).checked_add(byte_offset as u32)?;
        let base_i32 = i32::try_from(base).ok()?;
        let bytes = self.read_wasm_memory(base_i32, 8)?;
        let arr: [u8; 8] = bytes.try_into().ok()?;
        Some(i64::from_le_bytes(arr))
    }

    /// Write `bytes` into WASM linear memory at `ptr`.
    ///
    /// Returns `true` on success, `false` if `ptr` is negative, if the module
    /// has no exported `"memory"`, or if the write range exceeds memory size.
    pub fn write_wasm_memory(&mut self, ptr: i32, bytes: &[u8]) -> bool {
        if ptr < 0 {
            return false;
        }
        let memory = match self.instance.get_memory(&mut self.store, "memory") {
            Some(m) => m,
            None => return false,
        };
        memory.write(&mut self.store, ptr as usize, bytes).is_ok()
    }

    /// Invoke an exported function and decode its return value as a `StructuredValue`.
    ///
    /// This is the typed ABI entry point.  It calls `invoke`, maps the raw
    /// `RuntimeValue` to a base integer, reads the full WASM linear memory for
    /// pointer-based decoding, then delegates to `ValueDecoder::decode`.
    ///
    /// `layout` must match the return type of the export as produced by the
    /// compiler (see `WasmArtifact::export_types`).
    pub fn invoke_typed(
        &mut self,
        export_name: &str,
        args: &[RuntimeArg],
        layout: &ValueLayout,
    ) -> RuntimeResult<StructuredValue> {
        let raw = self.invoke(export_name, args)?;
        let raw_i64 = match raw {
            RuntimeValue::I64(v) => v,
            RuntimeValue::I32(v) => v as i64,
            RuntimeValue::F64(f) => return Ok(StructuredValue::Float(f)),
            RuntimeValue::Unit => return Ok(StructuredValue::Unit),
        };
        let memory_size = self.wasm_memory_size();
        let memory = self.read_wasm_memory(0, memory_size).unwrap_or_default();
        Ok(ValueDecoder::decode(layout, raw_i64, &memory))
    }

    /// Return the byte size of the exported `"memory"`, or 0 if none.
    fn wasm_memory_size(&mut self) -> usize {
        self.instance
            .get_memory(&mut self.store, "memory")
            .map(|m| m.data_size(&self.store))
            .unwrap_or(0)
    }

    /// Invoke an exported WASM function asynchronously.
    ///
    /// Wraps the blocking [`invoke`](Self::invoke) call in
    /// [`tokio::task::block_in_place`], allowing callers to `.await` the
    /// result without blocking the tokio multi-thread executor.
    ///
    /// WASM execution itself is always synchronous.  The async wrapper yields
    /// cooperative control to the scheduler while the blocking call runs on
    /// the current thread, keeping other tasks responsive.
    ///
    /// # Mode note
    ///
    /// This method corresponds to [`CapabilityCallMode::Async`].  For
    /// synchronous invocations use [`invoke`](Self::invoke) directly.
    ///
    /// # Errors
    ///
    /// Propagates the same errors as [`invoke`](Self::invoke): missing
    /// exports, arity mismatches, and WASM traps.
    ///
    /// # Panics
    ///
    /// Panics if called from a tokio `current_thread` runtime that cannot
    /// support `block_in_place`.  Always use `flavor = "multi_thread"` or
    /// the default multi-thread scheduler.
    pub async fn invoke_async(
        &mut self,
        export_name: &str,
        args: &[RuntimeArg],
    ) -> RuntimeResult<RuntimeValue> {
        let export_name = export_name.to_string();
        let args = args.to_vec();
        // `Store<HostState>` is !Send — we cannot move it into spawn_blocking.
        // `block_in_place` runs the closure on the current (blocking-capable)
        // thread without requiring Send, satisfying Wasmtime's constraint.
        tokio::task::block_in_place(|| self.invoke(&export_name, &args))
    }

    /// Set the active distributed trace context for WASM-side capability calls.
    ///
    /// After calling this, every capability call dispatched through
    /// `dispatch_host_call` (WASM import `ail/host_call`) will create a child
    /// span derived from `ctx` and attach it to the `CapabilityCallExecuted`
    /// audit event for distributed trace correlation.
    ///
    /// Call with a fresh [`TraceContext`] at the start of each WASM invocation
    /// to correlate audit events with your tracing backend.
    pub fn set_trace_context(&mut self, ctx: TraceContext) {
        self.store.data_mut().trace_context = Some(ctx);
    }

    /// Return the currently active trace context, if any.
    ///
    /// Returns `None` if no context has been set via [`set_trace_context`].
    pub fn trace_context(&self) -> Option<TraceContext> {
        self.store.data().trace_context.clone()
    }

    /// Return a snapshot of the audit log from the shared log.
    pub fn audit_log(&self) -> AuditLog {
        self.store
            .data()
            .audit_log
            .lock()
            .expect("audit_log lock must not be poisoned")
            .clone()
    }
}

fn extern_kind(extern_: &Extern) -> &'static str {
    match extern_ {
        Extern::Func(_) => "func",
        Extern::Global(_) => "global",
        Extern::Memory(_) => "memory",
        Extern::Table(_) => "table",
        #[allow(unreachable_patterns)]
        _ => "extern",
    }
}

trait InvokeResultExt {
    fn unwrap_err_or_encoding_error(self, export_name: &str) -> RuntimeError;
}

impl InvokeResultExt for RuntimeResult<RuntimeValue> {
    fn unwrap_err_or_encoding_error(self, export_name: &str) -> RuntimeError {
        match self {
            Ok(_) => RuntimeError::EncodingError(format!(
                "export `{export_name}` failed bridge diagnostics"
            )),
            Err(err) => err,
        }
    }
}

impl fmt::Debug for RuntimeInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeInstance").finish_non_exhaustive()
    }
}
