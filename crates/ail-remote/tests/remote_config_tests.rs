// ── ail-remote / remote_config_tests ──────────────────────────────────────
//
// Integration tests for project remote config DTO validation and policy loading.

use ail_remote::{
    AgentIdentity, RemoteConfig, RemoteConfigError, RemoteEndpointConfig, RemoteSignerConfig,
    RemoteSignerRejectionReason, SignerTrustTier,
};
use ail_storage::codec::{CborCodec, ContentCodec};

const KEY_A: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const KEY_B: &str = "2222222222222222222222222222222222222222222222222222222222222222";

fn config_with_signer(public_key: &str) -> RemoteConfig {
    RemoteConfig {
        allowed_signers: vec![RemoteSignerConfig {
            public_key: public_key.to_string(),
            trust_tier: SignerTrustTier::Trusted,
            label: Some("remote-a".to_string()),
        }],
        remotes: vec![RemoteEndpointConfig {
            name: "origin".to_string(),
            endpoint: "https://remote.example/ail".to_string(),
        }],
    }
}

#[test]
fn valid_config_converts_to_signer_policy() {
    let policy = config_with_signer(KEY_A)
        .to_signer_policy()
        .expect("valid config must convert to signer policy");

    let identity = AgentIdentity {
        public_key: [0x11; 32],
        label: Some("submitted-label".to_string()),
    };
    let signer = policy
        .check_identity(&identity)
        .expect("configured signer must pass policy");

    assert_eq!(signer.public_key, [0x11; 32]);
    assert_eq!(signer.trust_tier, SignerTrustTier::Trusted);
    assert_eq!(signer.label.as_deref(), Some("remote-a"));
}

#[test]
fn empty_allowed_signers_defaults_to_deny_all() {
    let config = RemoteConfig::default();
    let policy = config
        .to_signer_policy()
        .expect("empty signer list is a valid deny-all config");

    let rejection = policy
        .check_identity(&AgentIdentity {
            public_key: [0x11; 32],
            label: None,
        })
        .expect_err("empty policy must reject unknown signer");

    assert_eq!(
        rejection.reason,
        RemoteSignerRejectionReason::SignerNotAllowed
    );
}

#[test]
fn unknown_allowed_signer_key_is_rejected_by_loaded_policy() {
    let policy = config_with_signer(KEY_A)
        .to_signer_policy()
        .expect("valid config must convert to signer policy");

    let rejection = policy
        .check_identity(&AgentIdentity {
            public_key: [0x22; 32],
            label: Some("unknown".to_string()),
        })
        .expect_err("unconfigured signer must be rejected");

    assert_eq!(rejection.public_key, [0x22; 32]);
    assert_eq!(rejection.label.as_deref(), Some("unknown"));
    assert_eq!(
        rejection.reason,
        RemoteSignerRejectionReason::SignerNotAllowed
    );
}

#[test]
fn invalid_allowed_signer_key_fails_validation() {
    let error = config_with_signer("not-a-32-byte-ed25519-public-key")
        .to_signer_policy()
        .expect_err("malformed public key must fail validation");

    assert!(matches!(error, RemoteConfigError::InvalidPublicKey { .. }));
}

#[test]
fn duplicate_allowed_signer_key_fails_validation() {
    let mut config = config_with_signer(KEY_A);
    config.allowed_signers.push(RemoteSignerConfig {
        public_key: KEY_A.to_uppercase(),
        trust_tier: SignerTrustTier::External,
        label: Some("duplicate".to_string()),
    });

    let error = config
        .to_signer_policy()
        .expect_err("duplicate signer keys must fail validation");

    assert!(matches!(
        error,
        RemoteConfigError::DuplicateAllowedSigner { .. }
    ));
}

#[test]
fn remote_config_json_shape_is_stable() {
    let json = serde_json::to_string(&config_with_signer(KEY_A)).expect("encode json");

    assert_eq!(
        json,
        r#"{"allowed_signers":[{"public_key":"1111111111111111111111111111111111111111111111111111111111111111","trust_tier":"Trusted","label":"remote-a"}],"remotes":[{"name":"origin","endpoint":"https://remote.example/ail"}]}"#
    );

    let decoded: RemoteConfig = serde_json::from_str(&json).expect("decode json");
    assert_eq!(decoded, config_with_signer(KEY_A));
}

#[test]
fn remote_config_cbor_roundtrip_is_stable() {
    let codec = CborCodec;
    let config = RemoteConfig {
        allowed_signers: vec![
            RemoteSignerConfig {
                public_key: KEY_A.to_string(),
                trust_tier: SignerTrustTier::Primary,
                label: Some("remote-a".to_string()),
            },
            RemoteSignerConfig {
                public_key: KEY_B.to_string(),
                trust_tier: SignerTrustTier::External,
                label: None,
            },
        ],
        remotes: vec![RemoteEndpointConfig {
            name: "origin".to_string(),
            endpoint: "https://remote.example/ail".to_string(),
        }],
    };

    let first = codec.encode(&config).expect("first cbor encode");
    let second = codec.encode(&config).expect("second cbor encode");
    assert_eq!(
        first, second,
        "identical config must encode deterministically"
    );

    let decoded: RemoteConfig = codec.decode(&first).expect("decode cbor");
    assert_eq!(decoded, config);
}
