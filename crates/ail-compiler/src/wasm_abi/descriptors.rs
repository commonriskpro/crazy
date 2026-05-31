use super::*;
use std::collections::BTreeSet;

// ── ABI versioning ────────────────────────────────────────────────────────

/// Current WASM ABI version.  Increment this when the typed-value layout
/// contract changes in a backward-incompatible way.
pub const ABI_VERSION: u32 = 1;

/// A versioned envelope for the per-export type descriptors emitted by the
/// compiler.  Callers that own a `WasmArtifact` can construct an
/// `AbiDescriptor` from `export_types` and pass it across a process boundary
/// (e.g. serialise to JSON) so the runtime can check compatibility before
/// invoking typed exports.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AbiDescriptor {
    /// Layout version.  Must equal [`ABI_VERSION`] for the current runtime to
    /// decode the exports without an upgrade path.
    pub abi_version: u32,
    /// Maps each exported function name to its [`WasmTypeDescriptor`].
    pub exports: BTreeMap<String, WasmTypeDescriptor>,
}

impl AbiDescriptor {
    /// Wrap `exports` with the current [`ABI_VERSION`].
    pub fn new(exports: BTreeMap<String, WasmTypeDescriptor>) -> Self {
        Self {
            abi_version: ABI_VERSION,
            exports,
        }
    }

    /// Returns `true` when this descriptor's version matches the current
    /// runtime's expected ABI version.
    pub fn is_compatible(&self) -> bool {
        self.abi_version == ABI_VERSION
    }

    /// Return each export's stable wire shape in canonical export-name order.
    ///
    /// This is intentionally descriptor-derived rather than backend-derived so
    /// release gates can compare ABI compatibility without decoding a concrete
    /// WASM module or native object.
    pub fn export_wire_shapes(&self) -> BTreeMap<String, WasmWireShape> {
        self.exports
            .iter()
            .map(|(name, descriptor)| (name.clone(), descriptor.wire_shape()))
            .collect()
    }

    /// Validate this descriptor before handing it to a runtime or release gate.
    ///
    /// This catches ABI drift that is otherwise easy to miss in backend parity
    /// tests: version skew, legacy graph-style export names, and ambiguous
    /// structured descriptors that a runtime cannot decode deterministically.
    pub fn validation_issues(&self) -> Vec<AbiDescriptorIssue> {
        let mut issues = Vec::new();
        if self.abi_version != ABI_VERSION {
            issues.push(AbiDescriptorIssue::IncompatibleVersion {
                expected: ABI_VERSION,
                actual: self.abi_version,
            });
        }

        for (export, descriptor) in &self.exports {
            if export.trim().is_empty() {
                issues.push(AbiDescriptorIssue::EmptyExportName);
            }
            if !export.trim().is_empty() && !is_stable_abi_identifier(export) {
                issues.push(AbiDescriptorIssue::InvalidExportName {
                    export: export.clone(),
                });
            }
            if export.starts_with("fn.") || export.starts_with("test.") {
                issues.push(AbiDescriptorIssue::LegacyGraphExportName {
                    export: export.clone(),
                });
            }
            descriptor.collect_validation_issues(export, &mut issues);
        }
        issues.sort();
        issues.dedup();
        issues
    }

    /// Returns `true` when the descriptor is version-compatible and has no
    /// structural ABI validation issues.
    pub fn is_valid_for_runtime(&self) -> bool {
        self.validation_issues().is_empty()
    }

    /// Validate this descriptor against the concrete WASM module boundary.
    ///
    /// `validation_issues` checks the descriptor itself. This method also
    /// compares the descriptor with exported function signatures, import names,
    /// and whether linear memory is available when pointer-based values cross
    /// the ABI boundary. Returned issues are sorted for deterministic release
    /// gates.
    pub fn validation_issues_for_module(&self, module: &AbiModuleShape) -> Vec<AbiDescriptorIssue> {
        let mut issues = self.validation_issues();

        for import in &module.imports {
            if !is_stable_abi_identifier(&import.module) || !is_stable_abi_identifier(&import.name)
            {
                issues.push(AbiDescriptorIssue::InvalidImportName {
                    module: import.module.clone(),
                    name: import.name.clone(),
                });
            }
        }

        for export in module.exports.keys() {
            if export.trim().is_empty() {
                issues.push(AbiDescriptorIssue::EmptyExportName);
            } else if !is_stable_abi_identifier(export) {
                issues.push(AbiDescriptorIssue::InvalidExportName {
                    export: export.clone(),
                });
            }
        }

        for (export, descriptor) in &self.exports {
            let Some(actual) = module.exports.get(export) else {
                issues.push(AbiDescriptorIssue::MissingExportFunction {
                    export: export.clone(),
                });
                continue;
            };

            for (index, actual_param) in actual.params.iter().copied().enumerate() {
                if actual_param != WasmScalarType::I64 {
                    issues.push(AbiDescriptorIssue::ArgumentTypeMismatch {
                        export: export.clone(),
                        index,
                        expected: WasmScalarType::I64,
                        actual: actual_param,
                    });
                }
            }

            let expected_result = descriptor.expected_result_slot();
            if actual.result != expected_result {
                issues.push(AbiDescriptorIssue::ReturnTypeMismatch {
                    export: export.clone(),
                    expected: expected_result,
                    actual: actual.result,
                });
            }

            if descriptor.requires_linear_memory_boundary() && !module.memory_exported {
                issues.push(AbiDescriptorIssue::MemoryBoundaryMismatch {
                    export: export.clone(),
                });
            }
        }

        issues.sort();
        issues.dedup();
        issues
    }

    /// Return stable, redacted diagnostics for descriptor-only validation.
    pub fn validation_diagnostics(&self) -> Vec<AbiDiagnostic> {
        redacted_diagnostics(self.validation_issues())
    }

    /// Return stable, redacted diagnostics for descriptor + module validation.
    pub fn validation_diagnostics_for_module(&self, module: &AbiModuleShape) -> Vec<AbiDiagnostic> {
        redacted_diagnostics(self.validation_issues_for_module(module))
    }
}

// ── WasmTypeDescriptor ───────────────────────────────────────────────────

/// Stable function signature shape at the WASM ABI boundary.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct AbiFunctionSignature {
    /// Scalar slots accepted by the exported function. Current AIL ABI lowering
    /// passes user values as `i64` slots.
    pub params: Vec<WasmScalarType>,
    /// Scalar slot returned by the exported function, or `None` for no result.
    pub result: Option<WasmScalarType>,
}

impl AbiFunctionSignature {
    pub fn new(params: Vec<WasmScalarType>, result: Option<WasmScalarType>) -> Self {
        Self { params, result }
    }
}

/// Stable imported function shape at the WASM ABI boundary.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct AbiImportShape {
    pub module: String,
    pub name: String,
    pub signature: AbiFunctionSignature,
}

impl AbiImportShape {
    pub fn new(
        module: impl Into<String>,
        name: impl Into<String>,
        signature: AbiFunctionSignature,
    ) -> Self {
        Self {
            module: module.into(),
            name: name.into(),
            signature,
        }
    }
}

/// Concrete WASM module boundary shape used to validate an [`AbiDescriptor`].
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AbiModuleShape {
    /// Exported function signatures keyed by exported function name.
    pub exports: BTreeMap<String, AbiFunctionSignature>,
    /// Imported function signatures.
    pub imports: Vec<AbiImportShape>,
    /// Whether linear memory is exported to the runtime for pointer decoding.
    pub memory_exported: bool,
}

impl AbiModuleShape {
    pub fn new(exports: BTreeMap<String, AbiFunctionSignature>) -> Self {
        Self {
            exports,
            imports: Vec::new(),
            memory_exported: false,
        }
    }
}

/// Stable validation issue for an ABI descriptor.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum AbiDescriptorIssue {
    IncompatibleVersion {
        expected: u32,
        actual: u32,
    },
    EmptyExportName,
    InvalidExportName {
        export: String,
    },
    LegacyGraphExportName {
        export: String,
    },
    EmptyRecordFields {
        export: String,
    },
    InvalidRecordField {
        export: String,
        field: String,
    },
    DuplicateRecordField {
        export: String,
        field: String,
    },
    EmptyVariantTags {
        export: String,
    },
    InvalidVariantTag {
        export: String,
        tag: String,
    },
    DuplicateVariantTag {
        export: String,
        tag: String,
    },
    UnsupportedTypeLayout {
        export: String,
        layout: AbiTypeLayout,
    },
    MissingExportFunction {
        export: String,
    },
    InvalidImportName {
        module: String,
        name: String,
    },
    ArgumentTypeMismatch {
        export: String,
        index: usize,
        expected: WasmScalarType,
        actual: WasmScalarType,
    },
    ReturnTypeMismatch {
        export: String,
        expected: Option<WasmScalarType>,
        actual: Option<WasmScalarType>,
    },
    MemoryBoundaryMismatch {
        export: String,
    },
}

/// Coarse layout category for stable ABI diagnostics.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum AbiTypeLayout {
    F64Scalar,
    EmptyTuple,
}

/// Stable redacted diagnostic code for ABI validation.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum AbiDiagnosticCode {
    IncompatibleVersion,
    EmptyExportName,
    InvalidExportName,
    LegacyGraphExportName,
    EmptyRecordFields,
    InvalidRecordField,
    DuplicateRecordField,
    EmptyVariantTags,
    InvalidVariantTag,
    DuplicateVariantTag,
    UnsupportedTypeLayout,
    MissingExportFunction,
    InvalidImportName,
    ArgumentTypeMismatch,
    ReturnTypeMismatch,
    MemoryBoundaryMismatch,
}

/// Stable, redacted ABI diagnostic safe for release logs.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct AbiDiagnostic {
    pub code: AbiDiagnosticCode,
    /// Redacted stable location, e.g. `export:4:0123abcd89abcdef`.
    pub location: String,
    /// Redacted stable context for fields/import names/actual-vs-expected slots.
    pub detail: String,
}

fn redacted_diagnostics(issues: Vec<AbiDescriptorIssue>) -> Vec<AbiDiagnostic> {
    let mut diagnostics: Vec<_> = issues.into_iter().map(AbiDiagnostic::from).collect();
    diagnostics.sort();
    diagnostics
}

impl From<AbiDescriptorIssue> for AbiDiagnostic {
    fn from(issue: AbiDescriptorIssue) -> Self {
        match issue {
            AbiDescriptorIssue::IncompatibleVersion { expected, actual } => Self {
                code: AbiDiagnosticCode::IncompatibleVersion,
                location: "abi_version".to_string(),
                detail: format!("expected={expected};actual={actual}"),
            },
            AbiDescriptorIssue::EmptyExportName => Self {
                code: AbiDiagnosticCode::EmptyExportName,
                location: "export:<empty>".to_string(),
                detail: "export name is empty".to_string(),
            },
            AbiDescriptorIssue::InvalidExportName { export } => Self {
                code: AbiDiagnosticCode::InvalidExportName,
                location: redact("export", &export),
                detail: "export name must be a stable ABI identifier".to_string(),
            },
            AbiDescriptorIssue::LegacyGraphExportName { export } => Self {
                code: AbiDiagnosticCode::LegacyGraphExportName,
                location: redact("export", &export),
                detail: "export name still uses a graph prefix".to_string(),
            },
            AbiDescriptorIssue::EmptyRecordFields { export } => Self {
                code: AbiDiagnosticCode::EmptyRecordFields,
                location: redact("export", &export),
                detail: "record layout has no fields".to_string(),
            },
            AbiDescriptorIssue::InvalidRecordField { export, field } => Self {
                code: AbiDiagnosticCode::InvalidRecordField,
                location: redact("export", &export),
                detail: redact("field", &field),
            },
            AbiDescriptorIssue::DuplicateRecordField { export, field } => Self {
                code: AbiDiagnosticCode::DuplicateRecordField,
                location: redact("export", &export),
                detail: redact("field", &field),
            },
            AbiDescriptorIssue::EmptyVariantTags { export } => Self {
                code: AbiDiagnosticCode::EmptyVariantTags,
                location: redact("export", &export),
                detail: "variant layout has no tags".to_string(),
            },
            AbiDescriptorIssue::InvalidVariantTag { export, tag } => Self {
                code: AbiDiagnosticCode::InvalidVariantTag,
                location: redact("export", &export),
                detail: redact("tag", &tag),
            },
            AbiDescriptorIssue::DuplicateVariantTag { export, tag } => Self {
                code: AbiDiagnosticCode::DuplicateVariantTag,
                location: redact("export", &export),
                detail: redact("tag", &tag),
            },
            AbiDescriptorIssue::UnsupportedTypeLayout { export, layout } => Self {
                code: AbiDiagnosticCode::UnsupportedTypeLayout,
                location: redact("export", &export),
                detail: format!("layout={layout:?}"),
            },
            AbiDescriptorIssue::MissingExportFunction { export } => Self {
                code: AbiDiagnosticCode::MissingExportFunction,
                location: redact("export", &export),
                detail: "descriptor export is missing from module exports".to_string(),
            },
            AbiDescriptorIssue::InvalidImportName { module, name } => Self {
                code: AbiDiagnosticCode::InvalidImportName,
                location: redact("import_module", &module),
                detail: redact("import_name", &name),
            },
            AbiDescriptorIssue::ArgumentTypeMismatch {
                export,
                index,
                expected,
                actual,
            } => Self {
                code: AbiDiagnosticCode::ArgumentTypeMismatch,
                location: redact("export", &export),
                detail: format!("param={index};expected={expected:?};actual={actual:?}"),
            },
            AbiDescriptorIssue::ReturnTypeMismatch {
                export,
                expected,
                actual,
            } => Self {
                code: AbiDiagnosticCode::ReturnTypeMismatch,
                location: redact("export", &export),
                detail: format!("expected={expected:?};actual={actual:?}"),
            },
            AbiDescriptorIssue::MemoryBoundaryMismatch { export } => Self {
                code: AbiDiagnosticCode::MemoryBoundaryMismatch,
                location: redact("export", &export),
                detail: "descriptor crosses linear memory but module does not export memory"
                    .to_string(),
            },
        }
    }
}

fn redact(kind: &str, value: &str) -> String {
    if value.is_empty() {
        return format!("{kind}:<empty>");
    }
    format!("{kind}:{}:{:016x}", value.len(), stable_hash64(value))
}

fn stable_hash64(value: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn is_stable_abi_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

/// Scalar WASM primitive types used in the type descriptor.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum WasmScalarType {
    I64,
    F64,
    I32,
}

/// Stable ABI-level transport shape for a [`WasmTypeDescriptor`].
///
/// This is coarser than the semantic type descriptor.  It captures how a value
/// crosses the backend boundary, which is the compatibility contract runtimes,
/// native parity checks, and release gates need to compare.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WasmWireShape {
    /// Value is returned directly in one scalar result slot.
    ScalarSlot,
    /// Text/bytes are packed as `(len << 32) | ptr` in one `i64` slot.
    PackedPtrLen,
    /// Structured values are written via the host-call result buffer.
    StructuredResultBuffer,
    /// Opaque runtime resource handle returned in one scalar slot.
    HandleSlot,
}

/// Describes the return type of an exported WASM function for use by the
/// runtime decoder when reconstructing a `StructuredValue` from linear memory.
///
/// Populated by `emit_wasm` into `WasmArtifact::export_types`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WasmTypeDescriptor {
    Scalar(WasmScalarType),
    /// A UTF-8 text value packed as `(len as i64) << 32 | (ptr as i64)` in
    /// the raw i64 WASM return slot.  The runtime unpacks this into
    /// `StructuredValue::Text { ptr, len }` without a separate memory read.
    Text,
    /// A raw byte buffer packed as `(len as i64) << 32 | (ptr as i64)` in
    /// the raw i64 WASM return slot.  Decoded to
    /// `StructuredValue::Bytes { ptr, len }` without a memory read.
    ///
    /// Unlike [`WasmTypeDescriptor::Text`], no UTF-8 assumption is made —
    /// the bytes are treated as opaque.  Used for capability operations that
    /// return binary payloads (e.g. serialised CBOR, cryptographic digests).
    Bytes,
    Record {
        fields: Vec<String>,
    },
    Variant {
        tags: Vec<String>,
    },
    Tuple(Vec<WasmTypeDescriptor>),
    List(Box<WasmTypeDescriptor>),
    Option(Box<WasmTypeDescriptor>),
    Result {
        ok: Box<WasmTypeDescriptor>,
        err: Box<WasmTypeDescriptor>,
    },
    Handle,
}

impl WasmTypeDescriptor {
    /// Return the stable backend wire shape used to transport this descriptor.
    pub fn wire_shape(&self) -> WasmWireShape {
        match self {
            WasmTypeDescriptor::Scalar(_) => WasmWireShape::ScalarSlot,
            WasmTypeDescriptor::Text | WasmTypeDescriptor::Bytes => WasmWireShape::PackedPtrLen,
            WasmTypeDescriptor::Record { .. }
            | WasmTypeDescriptor::Variant { .. }
            | WasmTypeDescriptor::Tuple(_)
            | WasmTypeDescriptor::List(_)
            | WasmTypeDescriptor::Option(_)
            | WasmTypeDescriptor::Result { .. } => WasmWireShape::StructuredResultBuffer,
            WasmTypeDescriptor::Handle => WasmWireShape::HandleSlot,
        }
    }

    /// Expected scalar result slot for the concrete WASM function.
    pub fn expected_result_slot(&self) -> Option<WasmScalarType> {
        match self {
            WasmTypeDescriptor::Scalar(scalar) => Some(*scalar),
            WasmTypeDescriptor::Text | WasmTypeDescriptor::Bytes | WasmTypeDescriptor::Handle => {
                Some(WasmScalarType::I64)
            }
            WasmTypeDescriptor::Record { .. }
            | WasmTypeDescriptor::Variant { .. }
            | WasmTypeDescriptor::Tuple(_)
            | WasmTypeDescriptor::List(_)
            | WasmTypeDescriptor::Option(_)
            | WasmTypeDescriptor::Result { .. } => Some(WasmScalarType::I32),
        }
    }

    /// Returns true when runtime decoding requires exported linear memory.
    pub fn requires_linear_memory_boundary(&self) -> bool {
        !matches!(
            self,
            WasmTypeDescriptor::Scalar(_) | WasmTypeDescriptor::Handle
        )
    }

    fn collect_validation_issues(&self, export: &str, issues: &mut Vec<AbiDescriptorIssue>) {
        match self {
            WasmTypeDescriptor::Scalar(WasmScalarType::F64) => {
                issues.push(AbiDescriptorIssue::UnsupportedTypeLayout {
                    export: export.to_string(),
                    layout: AbiTypeLayout::F64Scalar,
                });
            }
            WasmTypeDescriptor::Record { fields } => {
                if fields.is_empty() {
                    issues.push(AbiDescriptorIssue::EmptyRecordFields {
                        export: export.to_string(),
                    });
                }
                let mut seen = BTreeSet::new();
                for field in fields {
                    if !is_stable_abi_identifier(field) {
                        issues.push(AbiDescriptorIssue::InvalidRecordField {
                            export: export.to_string(),
                            field: field.clone(),
                        });
                    }
                    if !seen.insert(field) {
                        issues.push(AbiDescriptorIssue::DuplicateRecordField {
                            export: export.to_string(),
                            field: field.clone(),
                        });
                    }
                }
            }
            WasmTypeDescriptor::Variant { tags } => {
                if tags.is_empty() {
                    issues.push(AbiDescriptorIssue::EmptyVariantTags {
                        export: export.to_string(),
                    });
                }
                let mut seen = BTreeSet::new();
                for tag in tags {
                    if !is_stable_abi_identifier(tag) {
                        issues.push(AbiDescriptorIssue::InvalidVariantTag {
                            export: export.to_string(),
                            tag: tag.clone(),
                        });
                    }
                    if !seen.insert(tag) {
                        issues.push(AbiDescriptorIssue::DuplicateVariantTag {
                            export: export.to_string(),
                            tag: tag.clone(),
                        });
                    }
                }
            }
            WasmTypeDescriptor::Tuple(items) => {
                if items.is_empty() {
                    issues.push(AbiDescriptorIssue::UnsupportedTypeLayout {
                        export: export.to_string(),
                        layout: AbiTypeLayout::EmptyTuple,
                    });
                }
                for item in items {
                    item.collect_validation_issues(export, issues);
                }
            }
            WasmTypeDescriptor::List(item) | WasmTypeDescriptor::Option(item) => {
                item.collect_validation_issues(export, issues);
            }
            WasmTypeDescriptor::Result { ok, err } => {
                ok.collect_validation_issues(export, issues);
                err.collect_validation_issues(export, issues);
            }
            WasmTypeDescriptor::Scalar(WasmScalarType::I64 | WasmScalarType::I32)
            | WasmTypeDescriptor::Text
            | WasmTypeDescriptor::Bytes
            | WasmTypeDescriptor::Handle => {}
        }
    }
}
