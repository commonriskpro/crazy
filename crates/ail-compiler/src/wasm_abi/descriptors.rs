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
        issues
    }

    /// Returns `true` when the descriptor is version-compatible and has no
    /// structural ABI validation issues.
    pub fn is_valid_for_runtime(&self) -> bool {
        self.validation_issues().is_empty()
    }
}

// ── WasmTypeDescriptor ───────────────────────────────────────────────────

/// Stable validation issue for an ABI descriptor.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AbiDescriptorIssue {
    IncompatibleVersion { expected: u32, actual: u32 },
    EmptyExportName,
    InvalidExportName { export: String },
    LegacyGraphExportName { export: String },
    EmptyRecordFields { export: String },
    InvalidRecordField { export: String, field: String },
    DuplicateRecordField { export: String, field: String },
    EmptyVariantTags { export: String },
    InvalidVariantTag { export: String, tag: String },
    DuplicateVariantTag { export: String, tag: String },
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
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

    fn collect_validation_issues(&self, export: &str, issues: &mut Vec<AbiDescriptorIssue>) {
        match self {
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
            WasmTypeDescriptor::Scalar(_)
            | WasmTypeDescriptor::Text
            | WasmTypeDescriptor::Bytes
            | WasmTypeDescriptor::Handle => {}
        }
    }
}
