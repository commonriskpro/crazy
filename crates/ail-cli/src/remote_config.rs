// -- ail-cli::remote_config -------------------------------------------------
//
// Project-local loading for the transport-agnostic ail-remote RemoteConfig DTO.

use std::fmt;
use std::path::{Path, PathBuf};

use ail_remote::{RemoteConfig, RemoteConfigError, RemoteSignerPolicy};

pub const REMOTE_CONFIG_FILE: &str = "remote.json";

#[derive(Debug)]
pub enum RemoteConfigLoadError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    Invalid {
        path: PathBuf,
        source: RemoteConfigError,
    },
}

impl fmt::Display for RemoteConfigLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RemoteConfigLoadError::Io { path, source } => {
                write!(
                    f,
                    "failed to read remote config {}: {source}",
                    path.display()
                )
            }
            RemoteConfigLoadError::Json { path, source } => {
                write!(
                    f,
                    "failed to parse remote config {}: {source}",
                    path.display()
                )
            }
            RemoteConfigLoadError::Invalid { path, source } => {
                write!(f, "invalid remote config {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for RemoteConfigLoadError {}

pub fn remote_config_path(ail_dir: &Path) -> PathBuf {
    ail_dir.join(REMOTE_CONFIG_FILE)
}

pub fn parse_remote_config_json(
    path: PathBuf,
    contents: &str,
) -> Result<RemoteConfig, RemoteConfigLoadError> {
    let config: RemoteConfig =
        serde_json::from_str(contents).map_err(|source| RemoteConfigLoadError::Json {
            path: path.clone(),
            source,
        })?;
    config
        .validate()
        .map_err(|source| RemoteConfigLoadError::Invalid { path, source })?;
    Ok(config)
}

pub fn load_remote_config(ail_dir: &Path) -> Result<RemoteConfig, RemoteConfigLoadError> {
    let path = remote_config_path(ail_dir);
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RemoteConfig::default());
        }
        Err(source) => return Err(RemoteConfigLoadError::Io { path, source }),
    };

    parse_remote_config_json(path, &contents)
}

pub fn load_remote_signer_policy(
    ail_dir: &Path,
) -> Result<RemoteSignerPolicy, RemoteConfigLoadError> {
    let path = remote_config_path(ail_dir);
    load_remote_config(ail_dir)?
        .to_signer_policy()
        .map_err(|source| RemoteConfigLoadError::Invalid { path, source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ail_remote::{AgentIdentity, RemoteEndpointConfig, RemoteSignerConfig, SignerTrustTier};

    const KEY_A: &str = "1111111111111111111111111111111111111111111111111111111111111111";

    fn config_json(public_key: &str) -> String {
        serde_json::to_string(&RemoteConfig {
            allowed_signers: vec![RemoteSignerConfig {
                public_key: public_key.to_string(),
                trust_tier: SignerTrustTier::Trusted,
                label: Some("remote-a".to_string()),
            }],
            remotes: vec![RemoteEndpointConfig {
                name: "origin".to_string(),
                endpoint: "https://remote.example/ail".to_string(),
            }],
        })
        .expect("config must encode")
    }

    #[test]
    fn remote_config_missing_defaults_to_deny_all_policy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ail_dir = dir.path().join(".ail");
        std::fs::create_dir_all(&ail_dir).expect("create .ail");

        let policy = load_remote_signer_policy(&ail_dir).expect("missing config is valid");
        let rejection = policy
            .check_identity(&AgentIdentity {
                public_key: [0x11; 32],
                label: None,
            })
            .expect_err("missing config must deny unknown signers");

        assert_eq!(rejection.public_key, [0x11; 32]);
    }

    #[test]
    fn remote_config_valid_json_loads_policy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ail_dir = dir.path().join(".ail");
        std::fs::create_dir_all(&ail_dir).expect("create .ail");
        std::fs::write(remote_config_path(&ail_dir), config_json(KEY_A)).expect("write config");

        let policy = load_remote_signer_policy(&ail_dir).expect("valid config must load");
        let signer = policy
            .check_identity(&AgentIdentity {
                public_key: [0x11; 32],
                label: Some("submitted-label".to_string()),
            })
            .expect("configured signer must pass");

        assert_eq!(signer.label.as_deref(), Some("remote-a"));
    }

    #[test]
    fn remote_config_invalid_json_returns_parse_error() {
        let error = parse_remote_config_json(PathBuf::from(".ail/remote.json"), "not json")
            .expect_err("invalid JSON must fail");

        assert!(matches!(error, RemoteConfigLoadError::Json { .. }));
    }

    #[test]
    fn remote_config_invalid_signer_returns_config_error() {
        let error = parse_remote_config_json(
            PathBuf::from(".ail/remote.json"),
            &config_json("not-a-public-key"),
        )
        .expect_err("invalid config must fail validation");

        assert!(matches!(error, RemoteConfigLoadError::Invalid { .. }));
    }
}
