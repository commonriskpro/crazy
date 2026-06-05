// ── ail-runtime::host_dispatch::diagnostics ───────────────────────────────
//
// Stable redacted diagnostics for the WASM ↔ host bridge.
//
// These diagnostics are intentionally additive: existing RuntimeError values
// remain unchanged, while production callers can ask for deterministic issue
// descriptors that do not expose raw import/export names or Wasmtime messages.

use wasmtime::{Engine, ExternType, Module, ValType};

use crate::error::RuntimeError;
use crate::host_dispatch::values::RuntimeArg;

const HOST_IMPORT_MODULE: &str = "ail";
const HOST_CALL: &str = "host_call";
const HOST_CALL_WRITE: &str = "host_call_write";

// ── public diagnostic types ───────────────────────────────────────────────

/// Stable category for a WASM bridge diagnostic.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WasmBridgeDiagnosticKind {
    /// The module could not be validated or instantiated.
    InstantiationFailure,
    /// A WASM import is not provided by the runtime bridge.
    MissingImport,
    /// A requested export is absent from an instantiated module.
    MissingExport,
    /// A known import/export exists but does not match the supported ABI.
    AbiMismatch,
    /// An exported function trapped during invocation.
    Trap,
}

/// One redacted, deterministic WASM bridge diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WasmBridgeDiagnostic {
    /// Stable issue kind for grouping.
    pub kind: WasmBridgeDiagnosticKind,
    /// Deterministic key suitable for sorting, deduplication, and dashboards.
    pub diagnostic_key: String,
    /// Redacted subject shape, never the raw import/export label.
    pub subject: String,
    /// Stable classifier within the diagnostic kind.
    pub classification: String,
    /// Redacted human detail for operational triage.
    pub detail: String,
}

/// Error returned by the diagnosed invocation API.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WasmBridgeInvokeError {
    /// Existing runtime error callers would otherwise observe.
    pub source: RuntimeError,
    /// Stable redacted bridge issues for this failed invocation.
    pub diagnostics: Vec<WasmBridgeDiagnostic>,
}

impl std::fmt::Display for WasmBridgeInvokeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "wasm bridge invocation failed: {} diagnostic(s); source: {}",
            self.diagnostics.len(),
            self.source
        )
    }
}

impl std::error::Error for WasmBridgeInvokeError {}

// ── constructors ─────────────────────────────────────────────────────────

impl WasmBridgeDiagnostic {
    fn new(
        kind: WasmBridgeDiagnosticKind,
        subject: String,
        classification: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        let classification = classification.into();
        let detail = detail.into();
        let diagnostic_key = format!(
            "wasm_bridge/{kind:?}/{subject}/{classification}",
            kind = kind,
            subject = subject,
            classification = classification,
        );
        Self {
            kind,
            diagnostic_key,
            subject,
            classification,
            detail,
        }
    }

    pub(crate) fn instantiation_failure(bytes_len: usize, error: &str) -> Self {
        Self::new(
            WasmBridgeDiagnosticKind::InstantiationFailure,
            format!("module:bytes_len={bytes_len}"),
            classify_instantiation_error(error),
            redacted_error_detail(error),
        )
    }

    fn missing_import(module: &str, name: &str) -> Self {
        Self::new(
            WasmBridgeDiagnosticKind::MissingImport,
            import_subject(module, name),
            "import.unbound",
            "import is not provided by the runtime bridge",
        )
    }

    fn import_abi_mismatch(module: &str, name: &str, expected: &str, actual: &str) -> Self {
        Self::new(
            WasmBridgeDiagnosticKind::AbiMismatch,
            import_subject(module, name),
            "import.signature",
            format!("expected {expected}; actual {actual}"),
        )
    }

    pub(crate) fn missing_export(name: &str) -> Self {
        Self::new(
            WasmBridgeDiagnosticKind::MissingExport,
            export_subject(name),
            "export.missing",
            "requested export is not available",
        )
    }

    pub(crate) fn export_abi_mismatch(
        name: &str,
        classification: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self::new(
            WasmBridgeDiagnosticKind::AbiMismatch,
            export_subject(name),
            classification,
            detail,
        )
    }

    pub(crate) fn trap(name: &str, error: &RuntimeError) -> Self {
        let message = error.to_string();
        Self::new(
            WasmBridgeDiagnosticKind::Trap,
            export_subject(name),
            classify_trap(&message),
            redacted_error_detail(&message),
        )
    }
}

// ── module-level diagnostics ─────────────────────────────────────────────

pub(crate) fn diagnose_wasm_bridge_module(
    engine: &Engine,
    wasm: &[u8],
) -> Vec<WasmBridgeDiagnostic> {
    let module = match Module::new(engine, wasm) {
        Ok(module) => module,
        Err(err) => {
            return vec![WasmBridgeDiagnostic::instantiation_failure(
                wasm.len(),
                &err.to_string(),
            )];
        }
    };

    sort_wasm_bridge_diagnostics(
        module
            .imports()
            .filter_map(|import| {
                let module_name = import.module();
                let name = import.name();
                match expected_host_import_signature(module_name, name) {
                    None => Some(WasmBridgeDiagnostic::missing_import(module_name, name)),
                    Some(expected) => match import.ty() {
                        ExternType::Func(func_ty) => {
                            let actual = func_signature(func_ty.params(), func_ty.results());
                            (actual != expected).then(|| {
                                WasmBridgeDiagnostic::import_abi_mismatch(
                                    module_name,
                                    name,
                                    expected,
                                    &actual,
                                )
                            })
                        }
                        other => Some(WasmBridgeDiagnostic::import_abi_mismatch(
                            module_name,
                            name,
                            expected,
                            &extern_type_signature(&other),
                        )),
                    },
                }
            })
            .collect(),
    )
}

pub(crate) fn sort_wasm_bridge_diagnostics(
    mut diagnostics: Vec<WasmBridgeDiagnostic>,
) -> Vec<WasmBridgeDiagnostic> {
    diagnostics.sort_by(|left, right| left.diagnostic_key.cmp(&right.diagnostic_key));
    diagnostics.dedup_by(|left, right| left.diagnostic_key == right.diagnostic_key);
    diagnostics
}

// ── export/invocation diagnostics ────────────────────────────────────────

pub(crate) fn diagnose_func_abi(
    export_name: &str,
    params: &[ValType],
    results: &[ValType],
    args: &[RuntimeArg],
) -> Vec<WasmBridgeDiagnostic> {
    let mut diagnostics = Vec::new();

    if params.len() != args.len() {
        diagnostics.push(WasmBridgeDiagnostic::export_abi_mismatch(
            export_name,
            "export.arity",
            format!("expected {} args; actual {}", params.len(), args.len()),
        ));
    }

    if params.iter().any(|ty| !is_supported_scalar(ty)) {
        diagnostics.push(WasmBridgeDiagnostic::export_abi_mismatch(
            export_name,
            "export.param_type",
            format!(
                "unsupported params {}; supported params i32|i64|f64",
                val_types_signature(params.iter().cloned())
            ),
        ));
    }

    if results.len() > 1 {
        diagnostics.push(WasmBridgeDiagnostic::export_abi_mismatch(
            export_name,
            "export.result_arity",
            format!("expected at most one result; actual {}", results.len()),
        ));
    }

    if results.iter().any(|ty| !is_supported_scalar(ty)) {
        diagnostics.push(WasmBridgeDiagnostic::export_abi_mismatch(
            export_name,
            "export.result_type",
            format!(
                "unsupported results {}; supported results i32|i64|f64|unit",
                val_types_signature(results.iter().cloned())
            ),
        ));
    }

    sort_wasm_bridge_diagnostics(diagnostics)
}

// ── redaction/classification helpers ─────────────────────────────────────

fn expected_host_import_signature(module: &str, name: &str) -> Option<&'static str> {
    match (module, name) {
        (HOST_IMPORT_MODULE, HOST_CALL) => Some("(i32,i32,i32,i32,i32,i32)->i64"),
        (HOST_IMPORT_MODULE, HOST_CALL_WRITE) => Some("(i32,i32,i32,i32,i32,i32,i32,i32)->i32"),
        _ => None,
    }
}

fn import_subject(module: &str, name: &str) -> String {
    format!(
        "import:module={},name={}",
        redacted_label(module),
        redacted_label(name)
    )
}

fn export_subject(name: &str) -> String {
    format!("export:name={}", redacted_label(name))
}

fn redacted_label(value: &str) -> String {
    let hash = blake3::hash(value.as_bytes()).to_hex().to_string();
    format!("h{}:len{}", &hash[..12], value.len())
}

fn is_supported_scalar(ty: &ValType) -> bool {
    matches!(ty, ValType::I32 | ValType::I64 | ValType::F64)
}

fn func_signature(
    params: impl IntoIterator<Item = ValType>,
    results: impl IntoIterator<Item = ValType>,
) -> String {
    format!(
        "({})->{}",
        val_types_signature(params),
        val_types_signature(results)
    )
}

fn val_types_signature(types: impl IntoIterator<Item = ValType>) -> String {
    let parts = types
        .into_iter()
        .map(|ty| match ty {
            ValType::I32 => "i32".to_string(),
            ValType::I64 => "i64".to_string(),
            ValType::F32 => "f32".to_string(),
            ValType::F64 => "f64".to_string(),
            ValType::V128 => "v128".to_string(),
            ValType::Ref(reference) => format!("ref({reference:?})"),
        })
        .collect::<Vec<_>>();
    if parts.is_empty() {
        "unit".to_string()
    } else {
        parts.join(",")
    }
}

fn extern_type_signature(ty: &ExternType) -> String {
    match ty {
        ExternType::Func(func_ty) => func_signature(func_ty.params(), func_ty.results()),
        ExternType::Global(_) => "global".to_string(),
        ExternType::Memory(_) => "memory".to_string(),
        ExternType::Table(_) => "table".to_string(),
        other => format!("{other:?}"),
    }
}

fn classify_instantiation_error(error: &str) -> &'static str {
    let lower = error.to_ascii_lowercase();
    if lower.contains("unknown import") || lower.contains("unknown") && lower.contains("import") {
        "instantiate.missing_import"
    } else if lower.contains("type mismatch") || lower.contains("incompatible import type") {
        "instantiate.abi_mismatch"
    } else {
        // Any other `Module::new` failure means the bytes were rejected during
        // parsing, validation, or compilation (e.g. bad magic header, truncated
        // sections, invalid UTF-8 in name sections). These all collapse to the
        // stable `wasm.validation` bucket: the module could not be validated.
        "wasm.validation"
    }
}

pub(crate) fn classify_trap(error: &str) -> &'static str {
    let lower = error.to_ascii_lowercase();
    if lower.contains("unreachable") {
        "trap.unreachable"
    } else if lower.contains("out of fuel") || lower.contains("fuel") {
        "trap.out_of_fuel"
    } else if lower.contains("out of bounds") || lower.contains("memory") {
        "trap.memory_out_of_bounds"
    } else if lower.contains("integer divide by zero") || lower.contains("division by zero") {
        "trap.integer_division_by_zero"
    } else if lower.contains("stack overflow") {
        "trap.stack_overflow"
    } else {
        "trap.runtime"
    }
}

fn redacted_error_detail(error: &str) -> String {
    format!("error_shape={}", redacted_label(error))
}
