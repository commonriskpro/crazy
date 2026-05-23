#![no_main]

use ail_runtime::schema::{
    CapabilityErrorSchema, CapabilityInputSchema, CapabilityOutputSchema, CapabilitySchema,
    SchemaField,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let schema = CapabilitySchema::new(
        CapabilityInputSchema::new(vec![
            SchemaField::new("cart_id", "String"),
            SchemaField::option("coupon", vec![SchemaField::new("code", "String")]),
        ]),
        CapabilityOutputSchema::new(vec![SchemaField::result(
            "payment",
            vec![SchemaField::new("receipt_id", "String")],
            vec![SchemaField::new("reason", "String")],
        )]),
        CapabilityErrorSchema::new(vec!["PaymentDeclined".to_string()]),
    );

    // Arbitrary payloads must either validate or return SchemaValidationError.
    // The boundary parser must not panic on malformed keys, variants, or UTF-8.
    let _ = schema.input().validate(data);
    let _ = schema.output().validate(data);
});
