// ── ail-runtime::secret ──────────────────────────────────────────────────
//
// Secret provider abstraction, in-memory vault, and `secret.read` handler.
//
// Design:
//   `SecretProvider` — trait that decouples `SecretReadHandler` from any
//     specific vault backend.  The single required method is `resolve`, which
//     maps a vault path to its secret bytes.  Future adapters (HashiCorp Vault,
//     AWS Secrets Manager, etc.) implement this trait without touching handler
//     or host code.
//
//   `SecretVault` — in-memory `SecretProvider` implementation.  Maps vault
//     paths to secret bytes.  Debug output is intentionally redacted; secret
//     values are never exposed through Display, Debug, or any log-facing path.
//
//   `SecretReadHandler` — a `Handler` implementation that serves
//     `secret.read:<secret_id>` capabilities.  It resolves the logical
//     secret ID through the profile's `secrets_mapping` (id → vault_path),
//     then fetches the value from any `SecretProvider`.
//
// Security invariants:
//   1. `SecretVault` values MUST NOT appear in `Debug` or `Display`.
//   2. Handlers return raw bytes; callers must not log those bytes.
//   3. The audit infrastructure records only BLAKE3 hashes of payloads,
//      so secret values never appear in audit events (verified by existing
//      audit infrastructure in host.rs / host_dispatch.rs).
//   4. An unmapped secret ID or a missing vault path returns
//      `HostError::CapabilityDenied`, not a more informative error, to
//      avoid leaking which secrets exist.
//
// Usage (in-memory vault):
//   ```rust,ignore
//   use std::sync::Arc;
//   use ail_runtime::secret::{SecretVault, SecretReadHandler};
//   use ail_runtime::profile::SecretEntry;
//
//   let mut vault = SecretVault::new();
//   vault.insert("prod/stripe-key", b"sk_live_abc123");
//
//   let mapping = vec![SecretEntry {
//       secret_id: "StripeApiKey".to_string(),
//       vault_path: "prod/stripe-key".to_string(),
//   }];
//
//   let handler = SecretReadHandler::new(mapping, Arc::new(vault));
//   let host = RuntimeHost::new().with_handler(Arc::new(handler));
//   ```
//
// Usage (custom provider):
//   ```rust,ignore
//   use ail_runtime::secret::SecretProvider;
//
//   struct MyVaultClient { /* ... */ }
//
//   impl SecretProvider for MyVaultClient {
//       fn resolve(&self, vault_path: &str) -> Option<Vec<u8>> {
//           // call external vault API
//           todo!()
//       }
//   }
//
//   let handler = SecretReadHandler::new(mapping, Arc::new(MyVaultClient { /* ... */ }));
//   ```

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::abi::{HostError, HostResult};
use crate::handler::Handler;
use crate::profile::{CapabilityId, SecretEntry};

// ── SecretProvider ────────────────────────────────────────────────────────

/// Trait for backends that resolve vault paths to secret byte values.
///
/// The in-memory implementation is [`SecretVault`].  Future adapters (e.g.
/// HashiCorp Vault, AWS Secrets Manager) can implement this trait and be
/// passed to [`SecretReadHandler::new`] without any other code changes.
///
/// # Security contract
///
/// - Implementors MUST NOT log or expose the returned bytes through any
///   public interface.  Only BLAKE3 hashes of secret payloads may appear in
///   audit events.
/// - Implementors must be `Send + Sync` so the provider can be shared behind
///   an `Arc` across threads (required by Wasmtime host-function closures).
pub trait SecretProvider: Send + Sync {
    /// Resolve `vault_path` to its secret bytes.
    ///
    /// Returns `None` if the path is unknown to this provider.
    /// The returned bytes MUST NOT be written to logs or audit fields;
    /// the audit infrastructure accepts only hashes.
    fn resolve(&self, vault_path: &str) -> Option<Vec<u8>>;
}

// ── SecretVault ───────────────────────────────────────────────────────────

/// In-memory vault mapping vault paths to secret byte values.
///
/// Implements [`SecretProvider`].  Values are NEVER exposed through `Debug`
/// or `Display`.  This type is intentionally opaque to prevent accidental
/// secret leakage through logging or error messages.
///
/// # Construction
///
/// ```rust,ignore
/// let mut vault = SecretVault::new();
/// vault.insert("prod/stripe-key", b"sk_live_abc123");
/// ```
pub struct SecretVault {
    // vault_path → secret_bytes
    secrets: HashMap<String, Vec<u8>>,
}

impl SecretVault {
    /// Create an empty vault.
    pub fn new() -> Self {
        SecretVault {
            secrets: HashMap::new(),
        }
    }

    /// Register a secret at `vault_path` with the given byte value.
    ///
    /// Overwrites any previously stored value at the same path.
    pub fn insert(&mut self, vault_path: impl Into<String>, value: impl Into<Vec<u8>>) {
        self.secrets.insert(vault_path.into(), value.into());
    }

    /// Resolve a vault path to its secret bytes.
    ///
    /// Returns `None` if no secret is stored at `vault_path`.
    /// The returned slice MUST NOT be written to logs or audit fields;
    /// the audit infrastructure accepts only hashes.
    pub fn resolve(&self, vault_path: &str) -> Option<&[u8]> {
        self.secrets.get(vault_path).map(Vec::as_slice)
    }
}

impl Default for SecretVault {
    fn default() -> Self {
        Self::new()
    }
}

/// Intentionally redacted to prevent secret leakage through logging.
///
/// Debug output shows the number of registered paths but never their names
/// or values.
impl fmt::Debug for SecretVault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretVault([{} entries redacted])", self.secrets.len())
    }
}

impl SecretProvider for SecretVault {
    /// Resolve a vault path to an owned copy of its secret bytes.
    ///
    /// Returns `None` if the path is unknown.  The returned bytes inherit
    /// all security invariants documented on [`SecretProvider::resolve`].
    fn resolve(&self, vault_path: &str) -> Option<Vec<u8>> {
        self.secrets.get(vault_path).cloned()
    }
}

// ── SecretReadHandler ─────────────────────────────────────────────────────

/// Capability handler for `secret.read:<secret_id>` calls.
///
/// Constructed from a profile's `secrets_mapping` (logical secret IDs → vault
/// paths) and any [`SecretProvider`] implementation.  At construction time,
/// this handler builds a [`CapabilityId`] for each entry in the mapping
/// (`"secret.read:<secret_id>"`), so the runtime grants system can verify
/// the handler covers exactly the set of secrets declared in the profile.
///
/// # Provider abstraction
///
/// The handler accepts any `P: SecretProvider + 'static`.  Pass
/// `Arc::new(SecretVault::new())` for the in-memory implementation, or any
/// custom adapter implementing [`SecretProvider`] for external vault backends.
/// No other code changes are required when swapping providers.
///
/// # Resolution flow
///
/// ```text
/// capability = "secret.read:StripeApiKey"
///   → strip "secret.read:" prefix → secret_id = "StripeApiKey"
///   → find SecretEntry where secret_id == "StripeApiKey" → vault_path
///   → SecretProvider::resolve(vault_path) → secret bytes
///   → return bytes (never logged; audit records only the BLAKE3 hash)
/// ```
///
/// # Denial behaviour
///
/// Any failure (missing mapping entry, missing vault path) returns
/// `HostError::CapabilityDenied` without distinguishing which step
/// failed, to avoid leaking information about the vault layout.
pub struct SecretReadHandler {
    // Declared capabilities: one per mapping entry.
    caps: Vec<CapabilityId>,
    // (secret_id, vault_path) parallel to caps, same index ordering.
    mapping: Vec<(String, String)>,
    provider: Arc<dyn SecretProvider>,
}

/// Capability prefix used by all secret-read capabilities.
const SECRET_READ_PREFIX: &str = "secret.read:";

impl SecretReadHandler {
    /// Construct a handler from a list of secret entries and a secret provider.
    ///
    /// `secrets_mapping` is typically sourced from
    /// [`RuntimeProfile::secrets_mapping`](crate::profile::RuntimeProfile::secrets_mapping).
    ///
    /// `provider` can be any `SecretProvider + 'static`, including
    /// [`SecretVault`] (in-memory) or a custom external adapter.
    ///
    /// Each entry produces a capability with ID `"secret.read:<secret_id>"`.
    pub fn new<P: SecretProvider + 'static>(
        secrets_mapping: Vec<SecretEntry>,
        provider: Arc<P>,
    ) -> Self {
        let (caps, mapping): (Vec<_>, Vec<_>) = secrets_mapping
            .into_iter()
            .map(|entry| {
                let cap = CapabilityId::new(format!("{SECRET_READ_PREFIX}{}", entry.secret_id));
                let pair = (entry.secret_id, entry.vault_path);
                (cap, pair)
            })
            .unzip();

        SecretReadHandler {
            caps,
            mapping,
            provider,
        }
    }
}

/// Intentionally omits provider contents and capability names from debug output.
///
/// Exposing the `caps` list would reveal the enumeration of secret IDs
/// declared in the profile.  Only the count is shown.
impl fmt::Debug for SecretReadHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretReadHandler")
            .field("caps_count", &self.caps.len())
            .field("provider", &"[redacted]")
            .finish()
    }
}

impl Handler for SecretReadHandler {
    fn name(&self) -> &str {
        "secret.read"
    }

    fn capabilities(&self) -> &[CapabilityId] {
        &self.caps
    }

    fn handle(
        &self,
        capability: &CapabilityId,
        operation: &str,
        _payload: &[u8],
    ) -> HostResult<Vec<u8>> {
        // Only the "read" operation is supported by this handler.
        if operation != "read" {
            return Err(HostError::CapabilityDenied(
                "secret access denied".to_string(),
            ));
        }

        // Extract secret_id from "secret.read:<secret_id>".
        // Structural format errors are kept distinct from access denials.
        let cap_str = capability.as_str();
        let secret_id = match cap_str.strip_prefix(SECRET_READ_PREFIX) {
            Some(id) if !id.is_empty() => id,
            _ => {
                return Err(HostError::CapabilityDenied(
                    "invalid secret.read capability format".to_string(),
                ));
            }
        };

        // Resolve secret_id → vault_path via the mapping.
        // Both mapping-miss and vault-miss produce the same opaque denial to
        // prevent callers from probing which step failed (vault-layout oracle).
        let vault_path = self
            .mapping
            .iter()
            .find(|(id, _)| id == secret_id)
            .map(|(_, path)| path.as_str());

        let vault_path = match vault_path {
            Some(p) => p,
            None => {
                return Err(HostError::CapabilityDenied(
                    "secret access denied".to_string(),
                ));
            }
        };

        // Resolve vault_path → secret bytes via the provider.
        match self.provider.resolve(vault_path) {
            Some(bytes) => Ok(bytes),
            None => Err(HostError::CapabilityDenied(
                "secret access denied".to_string(),
            )),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_resolve_present() {
        let mut v = SecretVault::new();
        v.insert("path/to/key", b"supersecret".to_vec());
        assert_eq!(v.resolve("path/to/key"), Some(b"supersecret".as_slice()));
    }

    #[test]
    fn vault_resolve_absent() {
        let v = SecretVault::new();
        assert_eq!(v.resolve("nonexistent"), None);
    }

    #[test]
    fn vault_debug_is_redacted() {
        let mut v = SecretVault::new();
        v.insert("path/to/key", b"supersecret".to_vec());
        let debug = format!("{v:?}");
        assert!(
            !debug.contains("supersecret"),
            "secret value must not appear in Debug"
        );
        assert!(
            !debug.contains("path/to/key"),
            "vault path must not appear in Debug"
        );
        assert!(debug.contains("redacted"), "debug must mention redacted");
    }

    #[test]
    fn handler_debug_is_redacted() {
        let mut vault = SecretVault::new();
        vault.insert("p", b"v".to_vec());
        let mapping = vec![SecretEntry {
            secret_id: "MyKey".to_string(),
            vault_path: "p".to_string(),
        }];
        let h = SecretReadHandler::new(mapping, Arc::new(vault));
        let debug = format!("{h:?}");
        // Secret values and vault contents must not appear.
        assert!(
            !debug.contains("supersecret"),
            "secret value must be hidden"
        );
        // The provider inner should be hidden.
        assert!(debug.contains("redacted"), "debug must mention redacted");
        // Capability names (= secret IDs) must NOT appear; only the count.
        assert!(
            !debug.contains("MyKey"),
            "secret IDs must not appear in Debug output"
        );
        assert!(
            !debug.contains("secret.read:"),
            "capability names must not appear in Debug output"
        );
    }

    #[test]
    fn handler_resolves_mapped_secret() {
        let mut vault = SecretVault::new();
        vault.insert("prod/stripe", b"sk_live_abc".to_vec());
        let mapping = vec![SecretEntry {
            secret_id: "StripeKey".to_string(),
            vault_path: "prod/stripe".to_string(),
        }];
        let h = SecretReadHandler::new(mapping, Arc::new(vault));
        let cap = CapabilityId::new("secret.read:StripeKey");
        let result = h.handle(&cap, "read", b"").expect("should resolve");
        assert_eq!(result, b"sk_live_abc");
    }

    #[test]
    fn handler_denies_unmapped_secret() {
        let vault = SecretVault::new();
        let mapping = vec![]; // no entries
        let h = SecretReadHandler::new(mapping, Arc::new(vault));
        let cap = CapabilityId::new("secret.read:StripeKey");
        let err = h.handle(&cap, "read", b"").expect_err("should be denied");
        let msg = match &err {
            HostError::CapabilityDenied(m) => m.clone(),
            other => panic!("expected CapabilityDenied, got {other:?}"),
        };
        // Opaque message — must not contain the secret ID or any layout detail.
        assert_eq!(msg, "secret access denied");
        assert!(!msg.contains("StripeKey"), "must not leak secret ID");
        assert!(!msg.contains("prod/"), "must not leak vault path");
    }

    #[test]
    fn handler_denies_missing_vault_path() {
        let vault = SecretVault::new(); // empty vault
        let mapping = vec![SecretEntry {
            secret_id: "StripeKey".to_string(),
            vault_path: "prod/stripe".to_string(), // path not in vault
        }];
        let h = SecretReadHandler::new(mapping, Arc::new(vault));
        let cap = CapabilityId::new("secret.read:StripeKey");
        let err = h.handle(&cap, "read", b"").expect_err("should be denied");
        let msg = match &err {
            HostError::CapabilityDenied(m) => m.clone(),
            other => panic!("expected CapabilityDenied, got {other:?}"),
        };
        // Opaque message — must not contain the secret ID or vault path.
        assert_eq!(msg, "secret access denied");
        assert!(!msg.contains("StripeKey"), "must not leak secret ID");
        assert!(!msg.contains("prod/stripe"), "must not leak vault path");
    }

    #[test]
    fn handler_unmapped_and_missing_vault_denial_are_identical() {
        // Both denial paths must produce exactly the same error to prevent
        // callers from probing vault layout by comparing error messages.
        let cap_unmapped = CapabilityId::new("secret.read:Unknown");
        let cap_mapped = CapabilityId::new("secret.read:StripeKey");

        // Unmapped secret.
        let h_unmapped = SecretReadHandler::new(vec![], Arc::new(SecretVault::new()));
        let err_unmapped = h_unmapped
            .handle(&cap_unmapped, "read", b"")
            .expect_err("unmapped must be denied");

        // Mapped but missing from vault.
        let mapping = vec![SecretEntry {
            secret_id: "StripeKey".to_string(),
            vault_path: "prod/stripe".to_string(),
        }];
        let h_missing = SecretReadHandler::new(mapping, Arc::new(SecretVault::new()));
        let err_missing = h_missing
            .handle(&cap_mapped, "read", b"")
            .expect_err("missing vault path must be denied");

        assert_eq!(
            err_unmapped, err_missing,
            "denial errors must be identical to prevent vault-layout probing"
        );
    }

    #[test]
    fn handler_denies_wrong_operation() {
        let mut vault = SecretVault::new();
        vault.insert("prod/stripe", b"sk_live".to_vec());
        let mapping = vec![SecretEntry {
            secret_id: "StripeKey".to_string(),
            vault_path: "prod/stripe".to_string(),
        }];
        let h = SecretReadHandler::new(mapping, Arc::new(vault));
        let cap = CapabilityId::new("secret.read:StripeKey");
        // "write" is not a valid operation for this handler.
        let err = h
            .handle(&cap, "write", b"")
            .expect_err("wrong operation must be denied");
        assert!(
            matches!(err, HostError::CapabilityDenied(_)),
            "expected CapabilityDenied for wrong operation, got {err:?}"
        );
    }

    #[test]
    fn handler_denies_malformed_capability() {
        let vault = SecretVault::new();
        let mapping = vec![];
        let h = SecretReadHandler::new(mapping, Arc::new(vault));
        // No suffix after "secret.read:"
        let cap = CapabilityId::new("secret.read:");
        let err = h
            .handle(&cap, "read", b"")
            .expect_err("malformed cap should be denied");
        assert!(matches!(err, HostError::CapabilityDenied(_)));
    }

    #[test]
    fn handler_capabilities_match_mapping() {
        let vault = SecretVault::new();
        let mapping = vec![
            SecretEntry {
                secret_id: "KeyA".to_string(),
                vault_path: "a".to_string(),
            },
            SecretEntry {
                secret_id: "KeyB".to_string(),
                vault_path: "b".to_string(),
            },
        ];
        let h = SecretReadHandler::new(mapping, Arc::new(vault));
        let caps: Vec<&str> = h.capabilities().iter().map(|c| c.as_str()).collect();
        assert_eq!(caps, vec!["secret.read:KeyA", "secret.read:KeyB"]);
    }

    // ── SecretProvider trait: custom provider tests ───────────────────────

    #[test]
    fn handler_works_with_custom_provider() {
        // Custom provider that serves a single hard-coded secret.
        struct StaticProvider;
        impl SecretProvider for StaticProvider {
            fn resolve(&self, vault_path: &str) -> Option<Vec<u8>> {
                match vault_path {
                    "custom/key" => Some(b"custom_secret_value".to_vec()),
                    _ => None,
                }
            }
        }

        let mapping = vec![SecretEntry {
            secret_id: "CustomKey".to_string(),
            vault_path: "custom/key".to_string(),
        }];
        let h = SecretReadHandler::new(mapping, Arc::new(StaticProvider));
        let cap = CapabilityId::new("secret.read:CustomKey");
        let result = h
            .handle(&cap, "read", b"")
            .expect("custom provider must resolve");
        assert_eq!(result, b"custom_secret_value");
    }

    #[test]
    fn handler_custom_provider_denial_is_opaque() {
        // Provider that always returns None — simulates a vault that has no
        // secrets (e.g. wrong environment, permissions not yet provisioned).
        struct EmptyProvider;
        impl SecretProvider for EmptyProvider {
            fn resolve(&self, _vault_path: &str) -> Option<Vec<u8>> {
                None
            }
        }

        let mapping = vec![SecretEntry {
            secret_id: "AnyKey".to_string(),
            vault_path: "any/path".to_string(),
        }];
        let h = SecretReadHandler::new(mapping, Arc::new(EmptyProvider));
        let cap = CapabilityId::new("secret.read:AnyKey");
        let err = h
            .handle(&cap, "read", b"")
            .expect_err("empty provider must deny");
        match &err {
            HostError::CapabilityDenied(m) => {
                assert_eq!(m, "secret access denied", "denial message must be opaque");
                assert!(!m.contains("AnyKey"), "must not leak secret ID");
                assert!(!m.contains("any/path"), "must not leak vault path");
            }
            other => panic!("expected CapabilityDenied, got {other:?}"),
        }
    }

    #[test]
    fn custom_provider_denial_matches_vault_denial() {
        // Verify that a custom provider's denial and the vault's missing-path
        // denial produce exactly the same error — no oracle leak via error
        // message differences across provider implementations.
        struct NullProvider;
        impl SecretProvider for NullProvider {
            fn resolve(&self, _: &str) -> Option<Vec<u8>> {
                None
            }
        }

        let mapping = vec![SecretEntry {
            secret_id: "Key".to_string(),
            vault_path: "p".to_string(),
        }];

        // Denial from custom provider.
        let h_custom = SecretReadHandler::new(mapping.clone(), Arc::new(NullProvider));
        let cap = CapabilityId::new("secret.read:Key");
        let err_custom = h_custom
            .handle(&cap, "read", b"")
            .expect_err("null provider must deny");

        // Denial from empty SecretVault (missing vault path).
        let h_vault = SecretReadHandler::new(mapping, Arc::new(SecretVault::new()));
        let err_vault = h_vault
            .handle(&cap, "read", b"")
            .expect_err("empty vault must deny");

        assert_eq!(
            err_custom, err_vault,
            "denial must be identical regardless of provider implementation"
        );
    }
}
