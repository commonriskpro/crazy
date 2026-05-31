use super::*;

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
}

// ── WasmTypeDescriptor ───────────────────────────────────────────────────

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
}
