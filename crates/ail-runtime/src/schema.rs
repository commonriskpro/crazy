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
// `CapabilitySchema` composes the three sub-schemas and validates payloads
// at the capability boundary.
//
// `CapabilityDefinition` binds a schema to a capability ID so the runtime
// host can enforce schemas at call sites.
//
// Validation protocol (simple key-presence format):
//   Payloads are parsed as comma-separated `key=value` pairs.
//   Schema validation checks that every declared leaf field path is present as
//   a key. Nested record fields use dot paths such as `receipt.id` and
//   `receipt.risk.score`. Option fields require a tag key such as
//   `receipt.$tag=Some`; only `Some` payload fields are required. Result fields
//   use the same tag protocol with `Ok` and `Err` payload branches. This is the
//   minimal boundary protocol for this implementation; a full CBOR/JSON schema
//   validation can replace it without changing the `validate()` signature.

use crate::profile::CapabilityId;
use std::collections::HashMap;

// ── SchemaValidationError ─────────────────────────────────────────────────

/// Error returned when payload boundary validation fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaValidationError {
    /// Human-readable description of the validation failure.
    pub message: String,
}

impl std::fmt::Display for SchemaValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

// ── SchemaField ───────────────────────────────────────────────────────────

/// A single named field in a capability schema.
///
/// Carries a `name` (the field identifier) and a `type_name` (a string
/// representation of the type, e.g. `"String"`, `"u64"`, `"Money"`).
/// String representations are used to keep the schema layer decoupled from
/// the type system while still being human-readable.
///
/// Record fields can carry nested fields. Validation then requires each leaf
/// field path in the encoded payload, e.g. `receipt.id=rcpt-1` for a nested
/// field `id` under record field `receipt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaField {
    name: String,
    type_name: String,
    fields: Vec<SchemaField>,
    variants: Vec<SchemaVariant>,
}

/// A single tagged branch in a structured schema field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaVariant {
    tag: String,
    fields: Vec<SchemaField>,
}

impl SchemaVariant {
    /// Create a variant branch with optional payload fields.
    pub fn new(tag: impl Into<String>, fields: Vec<SchemaField>) -> Self {
        SchemaVariant {
            tag: tag.into(),
            fields,
        }
    }

    /// Variant tag, e.g. `Some`.
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// Payload fields required when this variant is selected.
    pub fn fields(&self) -> &[SchemaField] {
        &self.fields
    }
}

impl SchemaField {
    /// Create a new `SchemaField` with `name` and `type_name`.
    pub fn new(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        SchemaField {
            name: name.into(),
            type_name: type_name.into(),
            fields: Vec::new(),
            variants: Vec::new(),
        }
    }

    /// Create a record field with nested child fields.
    pub fn record(name: impl Into<String>, fields: Vec<SchemaField>) -> Self {
        SchemaField {
            name: name.into(),
            type_name: "Record".to_string(),
            fields,
            variants: Vec::new(),
        }
    }

    /// Create an option field. `Some` validates the provided payload fields;
    /// `None` requires no payload.
    pub fn option(name: impl Into<String>, some_fields: Vec<SchemaField>) -> Self {
        SchemaField {
            name: name.into(),
            type_name: "Option".to_string(),
            fields: Vec::new(),
            variants: vec![
                SchemaVariant::new("None", vec![]),
                SchemaVariant::new("Some", some_fields),
            ],
        }
    }

    /// Create a result field. `Ok` validates `ok_fields`; `Err` validates
    /// `err_fields`.
    pub fn result(
        name: impl Into<String>,
        ok_fields: Vec<SchemaField>,
        err_fields: Vec<SchemaField>,
    ) -> Self {
        SchemaField {
            name: name.into(),
            type_name: "Result".to_string(),
            fields: Vec::new(),
            variants: vec![
                SchemaVariant::new("Ok", ok_fields),
                SchemaVariant::new("Err", err_fields),
            ],
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

    /// Nested fields for structured record validation.
    pub fn fields(&self) -> &[SchemaField] {
        &self.fields
    }

    /// Variant branches for structured option-like validation.
    pub fn variants(&self) -> &[SchemaVariant] {
        &self.variants
    }
}

// ── payload validation helper ─────────────────────────────────────────────

/// Parse a `key=value,...` encoded payload and return field values by key.
///
/// This is the minimal boundary protocol: a comma-separated list of
/// `key=value` pairs.  Empty payloads and empty schemas are always valid.
fn parse_fields(payload: &[u8]) -> Result<HashMap<String, String>, SchemaValidationError> {
    let s = std::str::from_utf8(payload).map_err(|_| SchemaValidationError {
        message: "PayloadDecodeError: payload must be valid UTF-8".to_string(),
    })?;
    Ok(s.split(',')
        .filter_map(|pair| {
            let pair = pair.trim();
            if pair.is_empty() {
                None
            } else {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                let key = key.trim().to_string();
                if key.is_empty() {
                    None
                } else {
                    Some((key, value.trim().to_string()))
                }
            }
        })
        .collect())
}

/// Validate that all `required_fields` are present as keys in `payload`.
fn validate_fields(
    payload: &[u8],
    required_fields: &[SchemaField],
) -> Result<(), SchemaValidationError> {
    if required_fields.is_empty() {
        return Ok(());
    }
    let fields = parse_fields(payload)?;
    for field in required_fields {
        validate_field_path(&fields, field, "")?;
    }
    Ok(())
}

fn validate_field_path(
    fields: &HashMap<String, String>,
    field: &SchemaField,
    parent_path: &str,
) -> Result<(), SchemaValidationError> {
    let path = if parent_path.is_empty() {
        field.name().to_string()
    } else {
        format!("{parent_path}.{}", field.name())
    };

    if !field.variants().is_empty() {
        let tag_path = format!("{path}.$tag");
        let tag = fields.get(&tag_path).ok_or_else(|| SchemaValidationError {
            message: format!("PayloadDecodeError: missing required field `{tag_path}`"),
        })?;
        let variant = field
            .variants()
            .iter()
            .find(|variant| variant.tag() == tag)
            .ok_or_else(|| SchemaValidationError {
                message: format!("PayloadDecodeError: unknown variant `{tag}` for `{path}`"),
            })?;
        for nested in variant.fields() {
            validate_field_path(fields, nested, &path)?;
        }
        return Ok(());
    }

    if field.fields().is_empty() {
        if !fields.contains_key(&path) {
            return Err(SchemaValidationError {
                message: format!("PayloadDecodeError: missing required field `{path}`"),
            });
        }
        return Ok(());
    }

    for nested in field.fields() {
        validate_field_path(fields, nested, &path)?;
    }
    Ok(())
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

    /// Validate that `payload` contains all required input fields.
    ///
    /// An empty schema accepts any payload.
    ///
    /// # Errors
    ///
    /// Returns [`SchemaValidationError`] if a required field is absent from
    /// the payload.
    pub fn validate(&self, payload: &[u8]) -> Result<(), SchemaValidationError> {
        validate_fields(payload, &self.fields)
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

    /// Validate that `response` contains all declared output fields.
    ///
    /// An empty schema accepts any response.
    pub fn validate(&self, response: &[u8]) -> Result<(), SchemaValidationError> {
        validate_fields(response, &self.fields)
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

// ── CapabilityDefinition ──────────────────────────────────────────────────

/// A capability ID bound to its typed schema.
///
/// `CapabilityDefinition` attaches a [`CapabilitySchema`] to a
/// [`CapabilityId`] so the runtime host can look up and enforce schemas when
/// `call_capability` is invoked.
///
/// Per runtime.md §"Payload schemas":
/// > Todo payload de capability tiene schema explícito.
/// > El host valida boundary encoding/decoding con el Boundary Protocol.
///
/// # Example
///
/// ```rust
/// use ail_runtime::schema::{
///     CapabilityDefinition, CapabilityErrorSchema, CapabilityInputSchema,
///     CapabilityOutputSchema, CapabilitySchema, SchemaField,
/// };
/// use ail_runtime::profile::CapabilityId;
///
/// let def = CapabilityDefinition::new(
///     CapabilityId::new("payment.charge:PaymentProvider"),
///     CapabilitySchema::new(
///         CapabilityInputSchema::new(vec![SchemaField::new("amount_cents", "u64")]),
///         CapabilityOutputSchema::new(vec![SchemaField::new("receipt_id", "String")]),
///         CapabilityErrorSchema::new(vec!["PaymentDeclined".to_string()]),
///     ),
/// );
/// assert_eq!(def.capability().as_str(), "payment.charge:PaymentProvider");
/// ```
#[derive(Clone, Debug)]
pub struct CapabilityDefinition {
    capability: CapabilityId,
    schema: CapabilitySchema,
}

impl CapabilityDefinition {
    /// Bind `capability` to its `schema`.
    pub fn new(capability: CapabilityId, schema: CapabilitySchema) -> Self {
        CapabilityDefinition { capability, schema }
    }

    /// The capability this definition describes.
    pub fn capability(&self) -> &CapabilityId {
        &self.capability
    }

    /// The typed schema for this capability.
    pub fn schema(&self) -> &CapabilitySchema {
        &self.schema
    }
}
