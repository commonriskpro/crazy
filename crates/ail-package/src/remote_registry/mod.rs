// ── ail-package::remote_registry ─────────────────────────────────────────
//
// Remote registry client/server protocol types for the AIL package registry.
//
// # Design (docs/packages.md §Open design questions — Package registry protocol)
//
// This module defines the request/response protocol for the remote package
// registry operations:
//   - publish: submit a signed package to the registry
//   - fetch:   retrieve a signed package by name/version
//   - search:  list packages matching a query
//   - verify:  check package integrity and advisory status
//
// The protocol is message-oriented.  Messages are CBOR-serializable for
// transport over any byte stream.  Authentication uses Ed25519 signatures
// (via `SignedPackage`).
//
// # Dependency isolation
//
// This module does NOT import a network runtime.  Callers wire the transport.

mod client;
mod in_memory;
mod signed_publish;
mod types;

pub use client::RegistryClient;
pub use in_memory::{InMemoryError, InMemoryRegistryClient};
pub use signed_publish::publish_signed;
pub use types::{
    FetchRequest, FetchResponse, PublishRequest, PublishResponse, SearchRequest, SearchResponse,
    SearchResult, VerifyOutcome, VerifyRequest, VerifyResponse,
};

#[cfg(test)]
mod tests;
