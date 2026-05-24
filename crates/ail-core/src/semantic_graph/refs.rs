// ── ail-core::semantic_graph::refs ───────────────────────────────────────
//
// Typed newtype wrappers for semantic identities: `BlockRef`, `ContractRef`,
// `EffectRef`, `ProofObligationRef`, `RuntimeCheckRef`.
//
// Each newtype is `serde(transparent)` so its CBOR encoding is identical to
// a plain `String`, preserving backward compatibility with all existing
// fixtures.
//
// This module is private to `semantic_graph`; all public items are
// re-exported from `semantic_graph/mod.rs`.

use serde::{Deserialize, Serialize};

/// Typed identity for a block in the semantic graph.
///
/// Wraps an opaque string identifier (e.g., `"block_checkout_flow"`).
/// Using `#[serde(transparent)]` keeps the CBOR wire format identical to a
/// plain `String` for backward compatibility.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlockRef(pub String);

/// Typed identity for a contract associated with a semantic node.
///
/// Wraps an opaque string identifier (e.g., `"contract.checkout.payment"`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContractRef(pub String);

/// Typed identity for an effect associated with a semantic node.
///
/// Wraps an opaque string identifier (e.g., `"effect.database.read"`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EffectRef(pub String);

/// Typed identity for a proof obligation attached to a semantic node.
///
/// Wraps an opaque string identifier (e.g., `"proof.invariant.no_negative_balance"`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProofObligationRef(pub String);

/// Typed identity for a runtime-check assertion at a semantic node.
///
/// Wraps an opaque string identifier (e.g., `"rtcheck.null_guard.cart_id"`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuntimeCheckRef(pub String);
