// ── ail-runtime::schema ───────────────────────────────────────────────────
//
// Typed payload schemas for capability definitions (G29).
//
// Per runtime.md:
//   capability payment.charge:PaymentProvider {
//     input  PaymentChargeRequest
//     output Result<PaymentReceipt, PaymentError>
//     errors PaymentProviderUnavailable | PaymentDeclined
//   }
//
// `CapabilitySchema` composes the three sub-schemas.  The runtime uses these
// descriptors for boundary validation; they are not yet enforced at WASM call
// sites (that requires a full compiler/ABI integration — tracked separately).

// ── SchemaField ───────────────────────────────────────────────────────────

/// A single named field in a capability schema.
///
/// Carries a `name` (the field identifier) and a `type_name` (a string
/// representation of the type, e.g. `"String"`, `"u64"`, `"Money"`).
/// String representations are used to keep the schema layer decoupled from
/// the type system while still being human-readable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaField {
    name: String,
    type_name: String,
}

impl SchemaField {
    /// Create a new `SchemaField` with `name` and `type_name`.
    pub fn new(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        SchemaField {
            name: name.into(),
            type_name: type_name.into(),
        }
    }

    /// Field identifier (e.g. `"cart_id"`).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Type descriptor string (e.g. `"String"`, `"Money"`).
    pub fn type_name(&self) -> &str {
        &self.type_name
    }
}

// ── CapabilityInputSchema ─────────────────────────────────────────────────

/// Describes the input payload of a capability call.
///
/// Contains an ordered list of [`SchemaField`]s that describe the expected
/// fields of the encoded request payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityInputSchema {
    fields: Vec<SchemaField>,
}

impl CapabilityInputSchema {
    /// Construct from a list of schema fields.
    pub fn new(fields: Vec<SchemaField>) -> Self {
        CapabilityInputSchema { fields }
    }

    /// Ordered list of input fields.
    pub fn fields(&self) -> &[SchemaField] {
        &self.fields
    }
}

// ── CapabilityOutputSchema ────────────────────────────────────────────────

/// Describes the output payload of a successful capability call.
///
/// Contains an ordered list of [`SchemaField`]s for the response encoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityOutputSchema {
    fields: Vec<SchemaField>,
}

impl CapabilityOutputSchema {
    /// Construct from a list of schema fields.
    pub fn new(fields: Vec<SchemaField>) -> Self {
        CapabilityOutputSchema { fields }
    }

    /// Ordered list of output fields.
    pub fn fields(&self) -> &[SchemaField] {
        &self.fields
    }
}

// ── CapabilityErrorSchema ─────────────────────────────────────────────────

/// Describes the error variants that a capability call may produce.
///
/// Each variant is a string name (e.g. `"PaymentDeclined"`).  These map to
/// the typed error domain declared in the runtime doc:
///   `errors PaymentProviderUnavailable | PaymentDeclined`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityErrorSchema {
    variants: Vec<String>,
}

impl CapabilityErrorSchema {
    /// Construct from a list of error variant names.
    pub fn new(variants: Vec<String>) -> Self {
        CapabilityErrorSchema { variants }
    }

    /// Ordered list of error variant names.
    pub fn variants(&self) -> &[String] {
        &self.variants
    }
}

// ── CapabilitySchema (composite) ──────────────────────────────────────────

/// Complete typed schema for a capability — input, output, and error contracts.
///
/// Composes [`CapabilityInputSchema`], [`CapabilityOutputSchema`], and
/// [`CapabilityErrorSchema`] into a single descriptor.
///
/// # Example
///
/// ```rust
/// use ail_runtime::schema::{
///     CapabilityErrorSchema, CapabilityInputSchema, CapabilityOutputSchema,
///     CapabilitySchema, SchemaField,
/// };
///
/// let schema = CapabilitySchema::new(
///     CapabilityInputSchema::new(vec![SchemaField::new("cart_id", "String")]),
///     CapabilityOutputSchema::new(vec![SchemaField::new("order_id", "OrderId")]),
///     CapabilityErrorSchema::new(vec!["CartNotFound".to_string()]),
/// );
/// assert_eq!(schema.input().fields().len(), 1);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilitySchema {
    input: CapabilityInputSchema,
    output: CapabilityOutputSchema,
    errors: CapabilityErrorSchema,
}

impl CapabilitySchema {
    /// Construct a full capability schema from its three components.
    pub fn new(
        input: CapabilityInputSchema,
        output: CapabilityOutputSchema,
        errors: CapabilityErrorSchema,
    ) -> Self {
        CapabilitySchema {
            input,
            output,
            errors,
        }
    }

    /// Input payload schema.
    pub fn input(&self) -> &CapabilityInputSchema {
        &self.input
    }

    /// Output payload schema.
    pub fn output(&self) -> &CapabilityOutputSchema {
        &self.output
    }

    /// Error variants schema.
    pub fn errors(&self) -> &CapabilityErrorSchema {
        &self.errors
    }
}
