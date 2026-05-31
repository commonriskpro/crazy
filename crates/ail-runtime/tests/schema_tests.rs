// ── schema_tests.rs ──────────────────────────────────────────────────────
//
// TDD tests for ail-runtime typed payload schemas (G29).
// Written BEFORE implementation — these are the RED phase.

use ail_runtime::schema::{
    CapabilityErrorSchema, CapabilityInputSchema, CapabilityOutputSchema, CapabilitySchema,
    SchemaField,
};

// ── SchemaField ───────────────────────────────────────────────────────────

#[test]
fn schema_field_carries_name_and_type() {
    let f = SchemaField::new("cart_id", "String");
    assert_eq!(f.name(), "cart_id");
    assert_eq!(f.type_name(), "String");
}

#[test]
fn schema_field_debug_contains_name() {
    let f = SchemaField::new("amount", "Money");
    let debug = format!("{f:?}");
    assert!(debug.contains("amount"), "debug must contain field name");
}

#[test]
fn schema_field_option_carries_none_and_some_variants() {
    let f = SchemaField::option("receipt", vec![SchemaField::new("id", "String")]);

    assert_eq!(f.name(), "receipt");
    assert_eq!(f.type_name(), "Option");
    assert_eq!(f.variants().len(), 2);
    assert_eq!(f.variants()[0].tag(), "None");
    assert_eq!(f.variants()[1].tag(), "Some");
    assert_eq!(f.variants()[1].fields()[0].name(), "id");
}

#[test]
fn schema_field_result_carries_ok_and_err_variants() {
    let f = SchemaField::result(
        "payment",
        vec![SchemaField::new("receipt_id", "String")],
        vec![SchemaField::new("reason", "String")],
    );

    assert_eq!(f.name(), "payment");
    assert_eq!(f.type_name(), "Result");
    assert_eq!(f.variants().len(), 2);
    assert_eq!(f.variants()[0].tag(), "Ok");
    assert_eq!(f.variants()[0].fields()[0].name(), "receipt_id");
    assert_eq!(f.variants()[1].tag(), "Err");
    assert_eq!(f.variants()[1].fields()[0].name(), "reason");
}

// ── CapabilityInputSchema ─────────────────────────────────────────────────

#[test]
fn input_schema_stores_fields() {
    let schema = CapabilityInputSchema::new(vec![
        SchemaField::new("cart_id", "String"),
        SchemaField::new("amount", "Money"),
    ]);
    assert_eq!(schema.fields().len(), 2);
    assert_eq!(schema.fields()[0].name(), "cart_id");
    assert_eq!(schema.fields()[1].name(), "amount");
}

#[test]
fn empty_input_schema_is_valid() {
    let schema = CapabilityInputSchema::new(vec![]);
    assert!(schema.fields().is_empty());
}

#[test]
fn input_schema_rejects_non_utf8_payload() {
    let schema = CapabilityInputSchema::new(vec![SchemaField::new("cart_id", "String")]);

    let err = schema
        .validate(b"cart_id=cart-1,noise=\xFF")
        .expect_err("invalid UTF-8 payload must be rejected");

    assert!(err.message.contains("key=schema.payload.invalid_utf8"));
    assert!(err.message.contains("category=payload_decode"));
    assert!(
        err.message
            .contains("expected_shape=payload:utf8_key_value_pairs")
    );
    assert!(err.message.contains("actual_shape=payload:non_utf8_bytes"));
    assert!(
        !err.message.contains("cart-1"),
        "diagnostic must not leak payload values: {}",
        err.message
    );
}

#[test]
fn empty_input_schema_still_accepts_arbitrary_bytes() {
    let schema = CapabilityInputSchema::new(vec![]);

    schema
        .validate(b"\xFF\xFE")
        .expect("empty schema remains deny-list free and accepts any payload");
}

#[test]
fn input_schema_missing_field_uses_stable_redacted_diagnostic() {
    let schema = CapabilityInputSchema::new(vec![
        SchemaField::new("account_id", "String"),
        SchemaField::new("api_token", "String"),
    ]);

    let err = schema
        .validate(b"account_id=acct_123,secret=sk_live_abc123")
        .expect_err("missing required field must be rejected");

    assert!(err.message.contains("key=schema.mismatch.missing_field"));
    assert!(err.message.contains("category=missing_field"));
    assert!(
        err.message
            .contains("expected_shape=field:api_token:String")
    );
    assert!(err.message.contains("actual_shape=field:absent"));
    assert!(
        !err.message.contains("acct_123") && !err.message.contains("sk_live_abc123"),
        "diagnostic must not leak payload values: {}",
        err.message
    );
}

#[test]
fn input_schema_unknown_variant_redacts_actual_tag_value() {
    let schema = CapabilityInputSchema::new(vec![SchemaField::option(
        "receipt",
        vec![SchemaField::new("id", "String")],
    )]);

    let err = schema
        .validate(b"receipt.$tag=Bearer sk_live_abc123,receipt.id=rcpt-1")
        .expect_err("unknown variant must be rejected");

    assert!(err.message.contains("key=schema.mismatch.unknown_variant"));
    assert!(err.message.contains("category=unknown_variant"));
    assert!(
        err.message
            .contains("expected_shape=variant:receipt:Option:tags=None|Some")
    );
    assert!(
        err.message
            .contains("actual_shape=variant_tag:present_unknown")
    );
    assert!(
        !err.message.contains("Bearer")
            && !err.message.contains("sk_live_abc123")
            && !err.message.contains("rcpt-1"),
        "diagnostic must not leak tag or payload values: {}",
        err.message
    );
}

// ── CapabilityOutputSchema ────────────────────────────────────────────────

#[test]
fn output_schema_stores_fields() {
    let schema = CapabilityOutputSchema::new(vec![SchemaField::new("receipt_id", "String")]);
    assert_eq!(schema.fields().len(), 1);
    assert_eq!(schema.fields()[0].name(), "receipt_id");
}

#[test]
fn output_schema_rejects_non_utf8_payload() {
    let schema = CapabilityOutputSchema::new(vec![SchemaField::new("receipt_id", "String")]);

    let err = schema
        .validate(b"receipt_id=rcpt-1,noise=\xFF")
        .expect_err("invalid UTF-8 response must be rejected");

    assert!(err.message.contains("key=schema.payload.invalid_utf8"));
    assert!(err.message.contains("category=payload_decode"));
    assert!(
        !err.message.contains("rcpt-1"),
        "diagnostic must not leak response values: {}",
        err.message
    );
}

// ── CapabilityErrorSchema ─────────────────────────────────────────────────

#[test]
fn error_schema_stores_variants() {
    let schema = CapabilityErrorSchema::new(vec![
        "PaymentProviderUnavailable".to_string(),
        "PaymentDeclined".to_string(),
    ]);
    assert_eq!(schema.variants().len(), 2);
    assert_eq!(schema.variants()[0], "PaymentProviderUnavailable");
    assert_eq!(schema.variants()[1], "PaymentDeclined");
}

#[test]
fn empty_error_schema_is_valid() {
    let schema = CapabilityErrorSchema::new(vec![]);
    assert!(schema.variants().is_empty());
}

// ── CapabilitySchema (composite) ──────────────────────────────────────────

#[test]
fn capability_schema_composes_input_output_errors() {
    let input = CapabilityInputSchema::new(vec![SchemaField::new("cart_id", "String")]);
    let output = CapabilityOutputSchema::new(vec![SchemaField::new("order_id", "OrderId")]);
    let errors = CapabilityErrorSchema::new(vec!["CartNotFound".to_string()]);

    let schema = CapabilitySchema::new(input, output, errors);

    assert_eq!(schema.input().fields().len(), 1);
    assert_eq!(schema.output().fields().len(), 1);
    assert_eq!(schema.errors().variants().len(), 1);
}

#[test]
fn capability_schema_field_names_match_spec_example() {
    // From runtime.md: payment.charge:PaymentProvider
    //   input PaymentChargeRequest
    //   output Result<PaymentReceipt, PaymentError>
    //   errors PaymentProviderUnavailable | PaymentDeclined
    let input = CapabilityInputSchema::new(vec![
        SchemaField::new("payment_method_id", "String"),
        SchemaField::new("amount_cents", "u64"),
        SchemaField::new("currency", "String"),
    ]);
    let output = CapabilityOutputSchema::new(vec![
        SchemaField::new("receipt_id", "String"),
        SchemaField::new("charged_at_ms", "u64"),
    ]);
    let errors = CapabilityErrorSchema::new(vec![
        "PaymentProviderUnavailable".to_string(),
        "PaymentDeclined".to_string(),
    ]);

    let schema = CapabilitySchema::new(input, output, errors);
    assert_eq!(schema.input().fields().len(), 3);
    assert_eq!(schema.output().fields().len(), 2);
    assert_eq!(schema.errors().variants().len(), 2);
    assert_eq!(schema.errors().variants()[1], "PaymentDeclined");
}
