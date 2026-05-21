// ── ail-package::verification ─────────────────────────────────────────────
//
// `PackageVerificationReport` — summary verification metrics for a package.
//
// A verification report records how many exports were verified, how many
// effects were declared, and how many contracts were formally proven.
// It is hash-bound: the report itself is content-addressed and its hash
// is stored in the `LockfileEntry.verification_report_hash` field.
//
// See `docs/packages.md` §Package verification report for the full design.

use serde::{Deserialize, Serialize};

// ── PackageVerificationReport ─────────────────────────────────────────────

/// Summary verification metrics produced during a package release.
///
/// A `PackageVerificationReport` records the counts of verified exports,
/// declared effects, and proven contracts.  These counts let the verifier
/// and dependency resolver quickly assess whether a package release meets
/// the policy requirements for its declared `TrustLevel`.
///
/// See `docs/packages.md` §Package verification report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageVerificationReport {
    /// Number of exported symbols for which verification evidence was
    /// produced and accepted.
    pub exports_verified: u32,
    /// Number of effect tokens that are declared in the manifest and
    /// present in verification evidence.
    pub effects_declared: u32,
    /// Number of contract IDs for which formal proofs were produced.
    pub contracts_proven: u32,
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_report() -> PackageVerificationReport {
        PackageVerificationReport {
            exports_verified: 5,
            effects_declared: 3,
            contracts_proven: 2,
        }
    }

    // ── verification_report_cbor_round_trip ───────────────────────────────
    // Spec scenario: "PackageVerificationReport round-trips through CBOR"
    //   GIVEN a PackageVerificationReport with all fields set
    //   WHEN serialized to CBOR and deserialized
    //   THEN all fields are equal to the original
    #[test]
    fn verification_report_cbor_round_trip() {
        let original = sample_report();

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&original, &mut buf).expect("CBOR serialization must succeed");

        let decoded: PackageVerificationReport =
            ciborium::de::from_reader(buf.as_slice()).expect("CBOR deserialization must succeed");

        assert_eq!(decoded, original);
    }

    // ── verification_report_cbor_is_deterministic ─────────────────────────
    // TRIANGULATE: encoding the same report twice yields identical bytes.
    #[test]
    fn verification_report_cbor_is_deterministic() {
        let report = sample_report();

        let mut buf1 = Vec::new();
        ciborium::ser::into_writer(&report, &mut buf1).expect("first encode");

        let mut buf2 = Vec::new();
        ciborium::ser::into_writer(&report, &mut buf2).expect("second encode");

        assert_eq!(
            buf1, buf2,
            "identical inputs must produce identical CBOR bytes"
        );
    }

    // ── zero_counts_are_valid ─────────────────────────────────────────────
    // TRIANGULATE: a report with all-zero counts is valid and round-trips.
    #[test]
    fn zero_counts_are_valid() {
        let report = PackageVerificationReport {
            exports_verified: 0,
            effects_declared: 0,
            contracts_proven: 0,
        };

        let mut buf = Vec::new();
        ciborium::ser::into_writer(&report, &mut buf).expect("encode");
        let decoded: PackageVerificationReport =
            ciborium::de::from_reader(buf.as_slice()).expect("decode");

        assert_eq!(decoded.exports_verified, 0);
        assert_eq!(decoded.effects_declared, 0);
        assert_eq!(decoded.contracts_proven, 0);
    }
}
