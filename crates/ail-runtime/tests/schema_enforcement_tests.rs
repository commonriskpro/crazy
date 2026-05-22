// ── schema_enforcement_tests.rs ──────────────────────────────────────────
//
// TDD tests for:
//   1. Runtime payload/boundary schema validation at call sites (CRITICAL)
//   2. CapabilityDefinition: schema attachment to capability (WARNING)
//
// Per runtime.md §"Payload schemas":
//   Todo payload de capability tiene schema explícito: CapabilityInputSchema,
//   CapabilityOutputSchema, CapabilityErrorSchema.
//   El host valida boundary encoding/decoding con el Boundary Protocol.
//
// Per runtime.md §"Runtime checks":
//   runtime_checked only counts if check exists in verified artifact hash.

use ail_runtime::schema::{
    CapabilityDefinition, CapabilityErrorSchema, CapabilityInputSchema, CapabilityOutputSchema,
    CapabilitySchema, SchemaField, SchemaValidationError,
};
use ail_runtime::profile::CapabilityId;

// ── CapabilityDefinition: schema attachment ───────────────────────────────

#[test]
fn capability_definition_attaches_id_and_schema() {
    let schema = CapabilitySchema::new(
        CapabilityInputSchema::new(vec![SchemaField::new("cart_id", "String")]),
        CapabilityOutputSchema::new(vec![SchemaField::new("order_id", "OrderId")]),
        CapabilityErrorSchema::new(vec!["CartNotFound".to_string()]),
    );
    let cap_id = CapabilityId::new("database.read:Cart");
    let def = CapabilityDefinition::new(cap_id.clone(), schema);

    assert_eq!(def.capability().as_str(), "database.read:Cart");
    assert_eq!(def.schema().input().fields().len(), 1);
    assert_eq!(def.schema().output().fields().len(), 1);
    assert_eq!(def.schema().errors().variants().len(), 1);
}

#[test]
fn capability_definition_from_spec_example() {
    // From runtime.md:
    //   capability payment.charge:PaymentProvider {
    //     input  PaymentChargeRequest
    //     output Result<PaymentReceipt, PaymentError>
    //     errors PaymentProviderUnavailable | PaymentDeclined
    //   }
    let def = CapabilityDefinition::new(
        CapabilityId::new("payment.charge:PaymentProvider"),
        CapabilitySchema::new(
            CapabilityInputSchema::new(vec![
                SchemaField::new("payment_method_id", "String"),
                SchemaField::new("amount_cents", "u64"),
            ]),
            CapabilityOutputSchema::new(vec![SchemaField::new("receipt_id", "String")]),
            CapabilityErrorSchema::new(vec![
                "PaymentProviderUnavailable".to_string(),
                "PaymentDeclined".to_string(),
            ]),
        ),
    );

    assert_eq!(def.capability().as_str(), "payment.charge:PaymentProvider");
    assert_eq!(def.schema().errors().variants().len(), 2);
}

// ── Schema validation: CapabilityInputSchema::validate ────────────────────

#[test]
fn input_schema_validates_required_fields_present() {
    // Schema requires two fields: "amount_cents" and "currency"
    let schema = CapabilityInputSchema::new(vec![
        SchemaField::new("amount_cents", "u64"),
        SchemaField::new("currency", "String"),
    ]);

    // Payload with both fields present (JSON-like key presence simulation)
    // We use a simple serialized key-value format for testing:
    // "amount_cents=100,currency=USD"
    let payload = b"amount_cents=100,currency=USD";
    assert!(schema.validate(payload).is_ok());
}

#[test]
fn input_schema_rejects_payload_missing_required_field() {
    let schema = CapabilityInputSchema::new(vec![
        SchemaField::new("amount_cents", "u64"),
        SchemaField::new("currency", "String"),
    ]);

    // Missing "currency"
    let payload = b"amount_cents=100";
    let result = schema.validate(payload);
    assert!(result.is_err(), "missing required field must fail validation");
    let err = result.unwrap_err();
    assert!(
        err.message.contains("currency"),
        "error must name the missing field: {:?}",
        err
    );
}

#[test]
fn empty_input_schema_accepts_any_payload() {
    let schema = CapabilityInputSchema::new(vec![]);
    // Empty schema = no constraints; any payload is valid
    assert!(schema.validate(b"anything").is_ok());
    assert!(schema.validate(b"").is_ok());
}

#[test]
fn output_schema_validates_required_response_fields() {
    let schema = CapabilityOutputSchema::new(vec![SchemaField::new("receipt_id", "String")]);
    let response = b"receipt_id=rcpt-42";
    assert!(schema.validate(response).is_ok());
}

#[test]
fn output_schema_rejects_response_missing_field() {
    let schema = CapabilityOutputSchema::new(vec![SchemaField::new("receipt_id", "String")]);
    let response = b"order_id=ord-1"; // wrong field name
    let result = schema.validate(response);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.message.contains("receipt_id"),
        "error must name the missing field: {:?}",
        err
    );
}

#[test]
fn empty_output_schema_accepts_any_response() {
    let schema = CapabilityOutputSchema::new(vec![]);
    assert!(schema.validate(b"anything").is_ok());
}

// ── SchemaValidationError ─────────────────────────────────────────────────

#[test]
fn schema_validation_error_carries_message() {
    let err = SchemaValidationError {
        message: "missing field: amount_cents".to_string(),
    };
    assert!(err.message.contains("amount_cents"));
}

// ── RuntimeHost schema registry ───────────────────────────────────────────

#[test]
fn runtime_host_can_register_capability_definition() {
    use ail_runtime::host::RuntimeHost;

    let def = CapabilityDefinition::new(
        CapabilityId::new("database.read:Cart"),
        CapabilitySchema::new(
            CapabilityInputSchema::new(vec![SchemaField::new("cart_id", "String")]),
            CapabilityOutputSchema::new(vec![SchemaField::new("total", "Money")]),
            CapabilityErrorSchema::new(vec![]),
        ),
    );

    // Should compile and not panic
    let _host = RuntimeHost::new().with_capability_definition(def);
}

#[test]
fn capability_call_with_schema_passes_valid_payload() {
    use std::sync::Arc;
    use ail_runtime::host::RuntimeHost;
    use ail_runtime::manifest::{CapabilityManifest, blake3_hex_of};
    use ail_runtime::profile::{CapabilityGrant, ResourceLimits, RuntimeProfile};
    use ail_runtime::InMemoryHandler;

    let wasm = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    let cap_id = CapabilityId::new("database.read:Cart");

    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![cap_id.clone()],
    };
    let module_hash = blake3_hex_of(&wasm);
    let manifest_hash = manifest.blake3_hex().unwrap();
    let grant = CapabilityGrant {
        module: "test".to_string(),
        capability: cap_id.clone(),
    };
    let profile = RuntimeProfile::new(
        "test".to_string(),
        module_hash,
        "vr-hash".to_string(),
        manifest_hash,
        vec![grant],
        ResourceLimits { max_memory_bytes: None, max_fuel: None },
    );

    // Schema requires "cart_id" field in the payload
    let def = CapabilityDefinition::new(
        cap_id.clone(),
        CapabilitySchema::new(
            CapabilityInputSchema::new(vec![SchemaField::new("cart_id", "String")]),
            CapabilityOutputSchema::new(vec![]),
            CapabilityErrorSchema::new(vec![]),
        ),
    );

    let handler = Arc::new(InMemoryHandler::new(
        "test-handler",
        vec![cap_id.clone()],
        b"cart-data".to_vec(),
    ));

    let mut host = RuntimeHost::new()
        .with_handler(handler)
        .with_capability_definition(def);

    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    // Valid payload: contains the "cart_id" field
    let result = host.call_capability(&cap_id, "read", b"cart_id=42");
    assert!(result.is_ok(), "valid payload must pass schema check: {:?}", result.err());
}

#[test]
fn capability_call_with_schema_rejects_invalid_payload() {
    use std::sync::Arc;
    use ail_runtime::host::RuntimeHost;
    use ail_runtime::manifest::{CapabilityManifest, blake3_hex_of};
    use ail_runtime::profile::{CapabilityGrant, ResourceLimits, RuntimeProfile};
    use ail_runtime::InMemoryHandler;

    let wasm = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    let cap_id = CapabilityId::new("database.read:Cart");

    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![cap_id.clone()],
    };
    let module_hash = blake3_hex_of(&wasm);
    let manifest_hash = manifest.blake3_hex().unwrap();
    let grant = CapabilityGrant {
        module: "test".to_string(),
        capability: cap_id.clone(),
    };
    let profile = RuntimeProfile::new(
        "test".to_string(),
        module_hash,
        "vr-hash".to_string(),
        manifest_hash,
        vec![grant],
        ResourceLimits { max_memory_bytes: None, max_fuel: None },
    );

    // Schema requires "cart_id" field
    let def = CapabilityDefinition::new(
        cap_id.clone(),
        CapabilitySchema::new(
            CapabilityInputSchema::new(vec![SchemaField::new("cart_id", "String")]),
            CapabilityOutputSchema::new(vec![]),
            CapabilityErrorSchema::new(vec![]),
        ),
    );

    let handler = Arc::new(InMemoryHandler::new(
        "test-handler",
        vec![cap_id.clone()],
        b"cart-data".to_vec(),
    ));

    let mut host = RuntimeHost::new()
        .with_handler(handler)
        .with_capability_definition(def);

    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    // Invalid payload: missing "cart_id"
    let result = host.call_capability(&cap_id, "read", b"wrong_field=42");
    assert!(result.is_err(), "invalid payload must fail schema check");
    let err = result.unwrap_err();
    assert!(
        err.message.contains("PayloadDecodeError") || err.message.contains("cart_id") || err.message.contains("schema"),
        "error must indicate payload schema violation: {:?}",
        err
    );
}

#[test]
fn capability_call_without_registered_schema_passes_through() {
    use std::sync::Arc;
    use ail_runtime::host::RuntimeHost;
    use ail_runtime::manifest::{CapabilityManifest, blake3_hex_of};
    use ail_runtime::profile::{CapabilityGrant, ResourceLimits, RuntimeProfile};
    use ail_runtime::InMemoryHandler;

    let wasm = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    let cap_id = CapabilityId::new("event.emit:OrderPaid");

    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![cap_id.clone()],
    };
    let module_hash = blake3_hex_of(&wasm);
    let manifest_hash = manifest.blake3_hex().unwrap();
    let grant = CapabilityGrant {
        module: "test".to_string(),
        capability: cap_id.clone(),
    };
    let profile = RuntimeProfile::new(
        "test".to_string(),
        module_hash,
        "vr-hash".to_string(),
        manifest_hash,
        vec![grant],
        ResourceLimits { max_memory_bytes: None, max_fuel: None },
    );

    let handler = Arc::new(InMemoryHandler::new(
        "test-handler",
        vec![cap_id.clone()],
        b"ok".to_vec(),
    ));

    // No schema registered for this capability — any payload must pass through
    let mut host = RuntimeHost::new().with_handler(handler);

    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let result = host.call_capability(&cap_id, "emit", b"any payload");
    assert!(result.is_ok(), "no registered schema = payload passthrough");
}
