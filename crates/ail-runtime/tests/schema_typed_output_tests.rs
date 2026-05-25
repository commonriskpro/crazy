// ── schema_typed_output_tests.rs ─────────────────────────────────────────
//
// Tests for the schema-to-ValueLayout bridge introduced in Wave 14B.
//
// Goal: close the typed capability output gap by proving that a capability
// schema declaring `output: Bytes` can validate and structurally decode
// `SecretReadHandler` responses without leaking raw bytes, and that
// mismatched output schemas are rejected.
//
// Test matrix:
//
// SO-1 — schema_field_to_value_layout: Bytes field → ValueLayout::Bytes
// SO-2 — schema_field_to_value_layout: Text / String → ValueLayout::Text
// SO-3 — schema_field_to_value_layout: numeric primitives → ValueLayout::Scalar
// SO-4 — schema_field_to_value_layout: Handle → ValueLayout::Handle
// SO-5 — schema_field_to_value_layout: unknown type → None
// SO-6 — declared_value_layout: single Bytes field → Some(Bytes)
// SO-7 — declared_value_layout: empty schema → None
// SO-8 — declared_value_layout: multi-field schema → None
// SO-9 — declared_value_layout: single Text field → Some(Text)
// SO-10 — declared_value_layout: single Scalar field → Some(Scalar)
// SO-11 — declared_value_layout: domain type → None
// SO-12 — validate_bytes_response: Bytes schema accepts any byte slice
// SO-13 — validate_bytes_response: Bytes schema produces correct StructuredValue
// SO-14 — validate_bytes_response: Bytes schema accepts empty bytes
// SO-15 — validate_bytes_response: Text schema → error (not a Bytes schema)
// SO-16 — validate_bytes_response: empty schema → error
// SO-17 — validate_bytes_response: multi-field schema → error
// SO-18 — validate_bytes_response: domain-type schema → error
// SO-19 — integration: SecretReadHandler output validates against Bytes schema
// SO-20 — integration: Bytes schema vs Text schema → output accepted / rejected
//          (proves only Bytes-typed schema accepts raw handler bytes)
// SO-21 — security: validate_bytes_response length check without reading bytes
//          (ptr sentinel is 0, not a real WASM memory address)

use std::sync::Arc;

use ail_runtime::codec::{StructuredValue, ValueLayout};
use ail_runtime::handler::Handler;
use ail_runtime::profile::{CapabilityId, SecretEntry};
use ail_runtime::schema::{CapabilityOutputSchema, SchemaField, schema_field_to_value_layout};
use ail_runtime::secret::{SecretReadHandler, SecretVault};

// ── SO-1..SO-5: schema_field_to_value_layout ─────────────────────────────

#[test]
fn so1_bytes_field_maps_to_value_layout_bytes() {
    let field = SchemaField::new("secret_data", "Bytes");
    assert_eq!(
        schema_field_to_value_layout(&field),
        Some(ValueLayout::Bytes),
        "\"Bytes\" type name must map to ValueLayout::Bytes"
    );
}

#[test]
fn so2_text_and_string_map_to_value_layout_text() {
    let text_field = SchemaField::new("label", "Text");
    assert_eq!(
        schema_field_to_value_layout(&text_field),
        Some(ValueLayout::Text),
        "\"Text\" must map to ValueLayout::Text"
    );

    let string_field = SchemaField::new("name", "String");
    assert_eq!(
        schema_field_to_value_layout(&string_field),
        Some(ValueLayout::Text),
        "\"String\" must map to ValueLayout::Text"
    );
}

#[test]
fn so3_numeric_primitives_map_to_value_layout_scalar() {
    let numeric_types = [
        "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "i128", "u128", "Int", "Bool",
        "Scalar",
    ];
    for type_name in numeric_types {
        let field = SchemaField::new("n", type_name);
        assert_eq!(
            schema_field_to_value_layout(&field),
            Some(ValueLayout::Scalar),
            "numeric type \"{type_name}\" must map to ValueLayout::Scalar"
        );
    }
}

#[test]
fn so4_handle_maps_to_value_layout_handle() {
    let field = SchemaField::new("resource", "Handle");
    assert_eq!(
        schema_field_to_value_layout(&field),
        Some(ValueLayout::Handle),
        "\"Handle\" must map to ValueLayout::Handle"
    );
}

#[test]
fn so5_unknown_type_returns_none() {
    // Domain record types and unrecognized names must return None.
    for type_name in &[
        "PaymentReceipt",
        "OrderId",
        "Money",
        "Record",
        "Option",
        "Result",
        "",
    ] {
        let field = SchemaField::new("field", *type_name);
        assert_eq!(
            schema_field_to_value_layout(&field),
            None,
            "domain type \"{type_name}\" must return None"
        );
    }
}

// ── SO-6..SO-11: declared_value_layout ───────────────────────────────────

#[test]
fn so6_single_bytes_field_schema_declares_bytes_layout() {
    let schema = CapabilityOutputSchema::new(vec![SchemaField::new("data", "Bytes")]);
    assert_eq!(
        schema.declared_value_layout(),
        Some(ValueLayout::Bytes),
        "single Bytes field → declared_value_layout must be Some(Bytes)"
    );
}

#[test]
fn so7_empty_schema_declares_no_layout() {
    let schema = CapabilityOutputSchema::new(vec![]);
    assert_eq!(
        schema.declared_value_layout(),
        None,
        "empty schema must return None (no fields to derive a layout from)"
    );
}

#[test]
fn so8_multi_field_schema_declares_no_layout() {
    let schema = CapabilityOutputSchema::new(vec![
        SchemaField::new("ptr", "Bytes"),
        SchemaField::new("len", "u32"),
    ]);
    assert_eq!(
        schema.declared_value_layout(),
        None,
        "multi-field schema must return None (ambiguous layout)"
    );
}

#[test]
fn so9_single_text_field_schema_declares_text_layout() {
    let schema = CapabilityOutputSchema::new(vec![SchemaField::new("label", "String")]);
    assert_eq!(schema.declared_value_layout(), Some(ValueLayout::Text));
}

#[test]
fn so10_single_scalar_field_schema_declares_scalar_layout() {
    let schema = CapabilityOutputSchema::new(vec![SchemaField::new("count", "u64")]);
    assert_eq!(schema.declared_value_layout(), Some(ValueLayout::Scalar));
}

#[test]
fn so11_domain_type_schema_declares_no_layout() {
    // Domain types like "PaymentReceipt" are not simple ValueLayout-mappable.
    let schema = CapabilityOutputSchema::new(vec![SchemaField::new("receipt", "PaymentReceipt")]);
    assert_eq!(
        schema.declared_value_layout(),
        None,
        "domain type must return None — no direct ValueLayout mapping"
    );
}

// ── SO-12..SO-21: validate_bytes_response ────────────────────────────────

#[test]
fn so12_bytes_schema_accepts_any_byte_slice() {
    let schema = CapabilityOutputSchema::new(vec![SchemaField::new("data", "Bytes")]);

    // Non-empty opaque bytes.
    let response = b"\xde\xad\xbe\xef\x00\xff";
    assert!(
        schema.validate_bytes_response(response).is_ok(),
        "Bytes schema must accept any non-empty byte slice"
    );

    // UTF-8 string — fine for Bytes too (no UTF-8 assumption).
    let utf8 = b"hello world";
    assert!(schema.validate_bytes_response(utf8).is_ok());
}

#[test]
fn so13_bytes_schema_produces_correct_structured_value() {
    let schema = CapabilityOutputSchema::new(vec![SchemaField::new("data", "Bytes")]);
    let response = b"\xde\xad\xbe\xef";

    let sv = schema
        .validate_bytes_response(response)
        .expect("Bytes schema must produce Ok");

    assert_eq!(
        sv,
        StructuredValue::Bytes { ptr: 0, len: 4 },
        "validate_bytes_response must return Bytes {{ ptr: 0, len: response.len() }}"
    );
}

#[test]
fn so14_bytes_schema_accepts_empty_response() {
    // Empty byte slice is valid — a secret or capability can legitimately return
    // zero bytes.  The caller decides whether that is meaningful.
    let schema = CapabilityOutputSchema::new(vec![SchemaField::new("data", "Bytes")]);
    let sv = schema
        .validate_bytes_response(b"")
        .expect("Bytes schema must accept empty bytes");
    assert_eq!(sv, StructuredValue::Bytes { ptr: 0, len: 0 });
}

#[test]
fn so15_text_schema_rejects_bytes_validation() {
    let schema = CapabilityOutputSchema::new(vec![SchemaField::new("label", "String")]);
    let err = schema
        .validate_bytes_response(b"hello")
        .expect_err("Text schema must reject validate_bytes_response");
    assert!(
        err.message.contains("BytesOutputError"),
        "error must be tagged BytesOutputError: {:?}",
        err.message
    );
    assert!(
        err.message.contains("not declared as Bytes"),
        "error must explain that schema is not Bytes-typed: {:?}",
        err.message
    );
}

#[test]
fn so16_empty_schema_rejects_bytes_validation() {
    let schema = CapabilityOutputSchema::new(vec![]);
    let err = schema
        .validate_bytes_response(b"data")
        .expect_err("empty schema must reject validate_bytes_response");
    assert!(
        err.message.contains("BytesOutputError"),
        "error must be tagged BytesOutputError: {:?}",
        err.message
    );
}

#[test]
fn so17_multi_field_schema_rejects_bytes_validation() {
    let schema = CapabilityOutputSchema::new(vec![
        SchemaField::new("a", "Bytes"),
        SchemaField::new("b", "Bytes"),
    ]);
    let err = schema
        .validate_bytes_response(b"data")
        .expect_err("multi-field schema must reject validate_bytes_response");
    assert!(
        err.message.contains("BytesOutputError"),
        "error must be tagged BytesOutputError: {:?}",
        err.message
    );
}

#[test]
fn so18_domain_type_schema_rejects_bytes_validation() {
    let schema = CapabilityOutputSchema::new(vec![SchemaField::new("receipt", "PaymentReceipt")]);
    let err = schema
        .validate_bytes_response(b"data")
        .expect_err("domain-type schema must reject validate_bytes_response");
    assert!(
        err.message.contains("BytesOutputError"),
        "error must be tagged BytesOutputError: {:?}",
        err.message
    );
}

// ── SO-19..SO-21: integration with SecretReadHandler ─────────────────────

/// Build a minimal `SecretReadHandler` with one secret.
fn make_secret_handler(
    secret_id: &str,
    vault_path: &str,
    secret_bytes: &[u8],
) -> Arc<SecretReadHandler> {
    let mut vault = SecretVault::new();
    vault.insert(vault_path, secret_bytes.to_vec());
    let mapping = vec![SecretEntry {
        secret_id: secret_id.to_string(),
        vault_path: vault_path.to_string(),
    }];
    Arc::new(SecretReadHandler::new(mapping, Arc::new(vault)))
}

#[test]
fn so19_secret_read_handler_output_validates_against_bytes_schema() {
    // Prove the full schema-validated Bytes pattern for secret.read:
    //   1. A capability schema declares `output: Bytes`.
    //   2. SecretReadHandler returns raw bytes.
    //   3. validate_bytes_response accepts them and produces the correct
    //      StructuredValue without reading byte content.
    const SECRET_ID: &str = "DbPass";
    const VAULT_PATH: &str = "prod/db-pass";
    const SECRET: &[u8] = b"\xca\xfe\xba\xbe\x00\x01\x02\x03";

    let handler = make_secret_handler(SECRET_ID, VAULT_PATH, SECRET);
    let cap = CapabilityId::new("secret.read:DbPass");

    let raw_output = handler
        .handle(&cap, "read", b"")
        .expect("handler must resolve the mapped secret");

    // Schema declares output as a single Bytes field.
    let schema = CapabilityOutputSchema::new(vec![SchemaField::new("secret_data", "Bytes")]);

    let sv = schema
        .validate_bytes_response(&raw_output)
        .expect("Bytes schema must accept SecretReadHandler output");

    // The StructuredValue must carry ptr=0 (sentinel) and the correct byte count.
    assert_eq!(
        sv,
        StructuredValue::Bytes {
            ptr: 0,
            len: SECRET.len() as i32,
        },
        "validate_bytes_response must reflect the exact byte count of the handler output"
    );
}

#[test]
fn so20_text_schema_rejects_secret_read_handler_output() {
    // A capability that mistakenly declares `output: String` instead of
    // `output: Bytes` for secret.read — validate_bytes_response must reject it.
    const SECRET_ID: &str = "ApiKey";
    const VAULT_PATH: &str = "dev/api-key";
    const SECRET: &[u8] = b"sk_test_abc123";

    let handler = make_secret_handler(SECRET_ID, VAULT_PATH, SECRET);
    let cap = CapabilityId::new("secret.read:ApiKey");

    let raw_output = handler
        .handle(&cap, "read", b"")
        .expect("handler must resolve the mapped secret");

    // Schema declares output as Text/String — WRONG for raw byte responses.
    let text_schema = CapabilityOutputSchema::new(vec![SchemaField::new("data", "String")]);

    let err = text_schema
        .validate_bytes_response(&raw_output)
        .expect_err("Text schema must reject validate_bytes_response for raw bytes");

    assert!(
        err.message.contains("BytesOutputError"),
        "error must be tagged BytesOutputError: {:?}",
        err.message
    );
    assert!(
        err.message.contains("not declared as Bytes"),
        "error must explain the schema mismatch: {:?}",
        err.message
    );
}

#[test]
fn so21_security_validate_bytes_response_exposes_only_length() {
    // Security invariant: validate_bytes_response must NOT expose raw bytes.
    // The only observable output is `StructuredValue::Bytes { ptr: 0, len }`.
    // ptr is always 0 (sentinel); the actual bytes are never returned or logged.
    const SECRET: &[u8] = b"super_secret_value";

    let schema = CapabilityOutputSchema::new(vec![SchemaField::new("data", "Bytes")]);
    let sv = schema
        .validate_bytes_response(SECRET)
        .expect("must succeed");

    match sv {
        StructuredValue::Bytes { ptr, len } => {
            // ptr must be 0 — the sentinel; no real WASM memory pointer.
            assert_eq!(ptr, 0, "ptr must be 0 sentinel (not a WASM memory address)");
            // len must match the byte count — shape-only validation.
            assert_eq!(len as usize, SECRET.len(), "len must equal the byte count");
            // The actual bytes are NOT accessible from the StructuredValue alone.
            // The caller must hold the original response slice to read the content.
        }
        other => panic!("expected StructuredValue::Bytes, got {other:?}"),
    }
}
