// ── ail-compiler::core_ir::types ─────────────────────────────────────────
//
// Type system primitives of the Semantic Core IR.

use serde::{Deserialize, Serialize};

use super::primitives::ResourceMode;

// ── CoreType ──────────────────────────────────────────────────────────────

/// Type primitives of the Semantic Core IR.
///
/// Corresponds to `docs/core-ir.md §3 — Sistema de tipos`.
///
/// All variants are unit-like at this stage; parameterised types (e.g.
/// `List<T>`, `Option<T>`) will carry sub-`CoreType` payloads in a future
/// phase once type-parameter resolution is wired through the semantic graph.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoreType {
    /// `()` — the unit type; returned by functions with no meaningful value.
    Unit,
    /// `Never` — uninhabited type; represents divergence or impossible branches.
    Never,
    /// Boolean type.
    Bool,
    /// Signed integer type (platform-default width at this stage).
    Int,
    /// Unsigned integer type.
    UInt,
    /// Floating-point type (IEEE 754 double).
    Float,
    /// UTF-8 text / string type.
    Text,
    /// Opaque byte sequence.
    Bytes,
    /// Nominal product type (`{ field: Type, ... }`).
    Record,
    /// Nominal sum type (`CaseA | CaseB(Payload)`).
    Variant,
    /// Structural product type (positional: `(A, B, C)`).
    Tuple,
    /// Homogeneous ordered collection with element type.
    ///
    /// `List(Box::new(CoreType::Int))` represents `List<Int>`.
    List(Box<CoreType>),
    /// Key-value association (ordered by key for determinism).
    ///
    /// `Map(key_type, value_type)` — e.g., `Map<Text, Int>`.
    Map(Box<CoreType>, Box<CoreType>),
    /// Unordered unique-element collection.
    ///
    /// `Set(Box::new(CoreType::Int))` represents `Set<Int>`.
    Set(Box<CoreType>),
    /// Optional value — `Some(T) | None`.
    ///
    /// `Option(Box::new(CoreType::Bool))` represents `Option<Bool>`.
    Option(Box<CoreType>),
    /// Fallible value — `Ok(T) | Err(E)`.
    ///
    /// `Result(ok_type, err_type)` — e.g., `Result<Int, Text>`.
    Result(Box<CoreType>, Box<CoreType>),
    /// Function type `(Params) -> Return` with optional effect row.
    Function {
        /// Ordered parameter types.
        params: Vec<CoreType>,
        /// Return type.
        ret: Box<CoreType>,
        /// Named effects (e.g., `["IO", "State"]`).
        effects: Vec<String>,
    },
    /// External resource handle with an ownership mode.
    Handle {
        /// The resource type being wrapped.
        resource: Box<CoreType>,
        /// The ownership / linearity mode.
        mode: ResourceMode,
    },
    /// A base type refined by a logical predicate.
    Refinement {
        /// The base type being refined.
        base: Box<CoreType>,
        /// The predicate expression string.
        predicate: String,
    },
    /// Generic type parameter — carries an optional inner type.
    ///
    /// `Generic(Some(Box::new(CoreType::Int)))` represents `Generic<Int>`.
    /// `Generic(None)` is the fallback when the nominal is unrecognised
    /// or when type parameters have not been resolved yet.
    ///
    /// Corresponds to `docs/core-ir.md §3 — Generic<T>`.
    Generic(Option<Box<CoreType>>),

    // ── ola3-core-ir-types: new flat numeric and Unicode variants ─────────
    /// Arbitrary-precision decimal number type.
    Decimal,
    /// Existential type — a value whose type is hidden behind an interface.
    Existential,
    /// Unicode code point (scalar value, U+0000..U+10FFFF).
    CodePoint,
    /// A single user-perceived character cluster (grapheme cluster).
    Grapheme,
    /// Unicode normalized text with an explicit normalization form.
    ///
    /// The `String` payload carries the form name: `"NFC"`, `"NFD"`,
    /// `"NFKC"`, or `"NFKD"`.
    NormalizedText(String),
    /// Signed 32-bit integer (fixed-width platform machine type).
    Int32,
    /// Signed 64-bit integer (fixed-width platform machine type).
    Int64,
    /// Unsigned 32-bit integer.
    UInt32,
    /// Unsigned 64-bit integer.
    UInt64,
    /// Type representing a group of concurrent tasks (mirrors `CoreExpr::TaskGroup`).
    TaskGroup,

    // ── ola3-core-ir-types: new parameterized collection and boundary types ─
    /// Partial-update field type.
    ///
    /// `PatchField(Box::new(CoreType::Text))` represents `PatchField<Text>`.
    PatchField(Box<CoreType>),
    /// Fixed-capacity vector (size is a separate ConstParam, not carried here).
    ///
    /// `Vector(Box::new(CoreType::Float))` represents `Vector<Float, N>`.
    Vector(Box<CoreType>),
    /// Ordered (sorted) unique-element set.
    ///
    /// `OrderedSet(Box::new(CoreType::Int))` represents `OrderedSet<Int>`.
    OrderedSet(Box<CoreType>),
    /// Ordered (sorted) key-value map.
    ///
    /// `OrderedMap(key_type, value_type)` — e.g., `OrderedMap<Int, Text>`.
    OrderedMap(Box<CoreType>, Box<CoreType>),
    /// Fixed-length array.
    ///
    /// `Array(Box::new(CoreType::Int))` represents `Array<Int, N>`.
    Array(Box<CoreType>),
    /// Asynchronous task returning a value of the given type.
    ///
    /// `Task(Box::new(CoreType::Bool))` represents `Task<Bool>`.
    Task(Box<CoreType>),
    /// Asynchronous message channel.
    ///
    /// `Channel(Box::new(CoreType::Text))` represents `Channel<Text>`.
    Channel(Box<CoreType>),
    /// Opaque external (foreign) type, identified by name.
    ForeignType(String),
    /// An encoded representation of a value (e.g., `Encoded<Json>`).
    Encoded(String),
    /// A decoded/parsed value of the given type.
    Decoded(Box<CoreType>),

    // ── ola4-type-formalism: dyn dispatch and boundary schema ─────────────
    /// Dynamic interface dispatch type — `Dyn<Interface>`.
    ///
    /// The `String` payload carries the interface name, e.g. `"Repository<User>"`.
    /// Follows the same flat-String payload pattern as `ForeignType`, `NormalizedText`,
    /// and `Encoded`.
    Dyn(String),

    /// Explicit serialization schema name attached to a boundary value.
    ///
    /// `BoundarySchema("UserInputJsonSchema")` identifies the schema governing
    /// how a value crossing a boundary must be encoded/decoded.
    BoundarySchema(String),

    // ── doc-alignment: missing CoreType variants from core-ir.md ──────────
    /// Adapter contract type — wraps a foreign boundary with type-level
    /// contract metadata.
    ///
    /// Corresponds to `docs/core-ir.md §13 — AdapterContract`.
    /// The `String` payload carries the adapter name (e.g., `"StripePaymentAdapter"`).
    AdapterContract(String),
}
