// ── ail-package::handler ──────────────────────────────────────────────────
//
// `HandlerExport` — a handler exported by a package.
//
// A handler export declares that the package provides a concrete handler
// implementation for a given capability.  Exporting a handler is NOT the
// same as binding or granting it — the runtime profile must explicitly
// bind the handler and grant the capability.
//
// Rule: handler export != handler binding.
//
// # Determinism contract
//
// All fields use deterministic types (String, enum discriminant).
// CBOR serialization via `ciborium` is byte-deterministic.

use serde::{Deserialize, Serialize};

use crate::trust::TrustLevel;

// ── HandlerExport ─────────────────────────────────────────────────────────

/// A handler exported by a package, providing a concrete implementation
/// for a declared capability.
///
/// The `trust_level` field records the trust tier of the handler
/// implementation itself (which may differ from the package's overall trust).
///
/// See `docs/packages.md` §Capability exports for the full design.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandlerExport {
    /// Capability token this handler implements
    /// (e.g., `"payment.charge:PaymentProvider"`).
    pub capability: String,
    /// Qualified handler name (e.g., `"StripePayment"`).
    pub handler_name: String,
    /// Trust level of this handler implementation.
    pub trust_level: TrustLevel,
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_handler() -> HandlerExport {
        HandlerExport {
            capability: "payment.charge:PaymentProvider".to_string(),
            handler_name: "StripePayment".to_string(),
            trust_level: TrustLevel::Assumed,
        }
    }

    // ── handler_export_cbor_round_trip ────────────────────────────────────
    // Spec scenario: "HandlerExport round-trips through CBOR"
    //   GIVEN a HandlerExport with all fields set
    //   WHEN serialized to CBOR and deserialized
    //   THEN all fields are equal to the original
    #[test]
    fn handler_export_cbor_round_trip() {
        let original = sample_handler();

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&original, &mut buf).expect("CBOR serialization must succeed");

        let decoded: HandlerExport =
            ciborium::de::from_reader(buf.as_slice()).expect("CBOR deserialization must succeed");

        assert_eq!(decoded, original);
    }

    // ── handler_export_cbor_is_deterministic ──────────────────────────────
    // TRIANGULATE: encoding the same value twice yields identical bytes.
    #[test]
    fn handler_export_cbor_is_deterministic() {
        let handler = sample_handler();

        let mut buf1 = Vec::new();
        ciborium::ser::into_writer(&handler, &mut buf1).expect("first encode");

        let mut buf2 = Vec::new();
        ciborium::ser::into_writer(&handler, &mut buf2).expect("second encode");

        assert_eq!(
            buf1, buf2,
            "identical inputs must produce identical CBOR bytes"
        );
    }

    // ── verified_handler_trust_survives_round_trip ────────────────────────
    // TRIANGULATE: TrustLevel::Verified survives CBOR round-trip.
    #[test]
    fn verified_handler_trust_survives_round_trip() {
        let handler = HandlerExport {
            capability: "io.read:Filesystem".to_string(),
            handler_name: "NativeReader".to_string(),
            trust_level: TrustLevel::Verified,
        };

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&handler, &mut buf).expect("encode");
        let decoded: HandlerExport = ciborium::de::from_reader(buf.as_slice()).expect("decode");

        assert_eq!(decoded.trust_level, TrustLevel::Verified);
    }
}
