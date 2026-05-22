/// ACL version migrators: trait, concrete implementations, and migration chain.
pub mod acl_migrator;
pub mod apply;
pub mod canonical;
/// Typed ChangeSet model, canonicalization, and atomic apply for `ail-change`.
pub mod model;
/// Op-schema validation layer: checks required/optional args per op verb.
pub mod op_schema;
/// Line-oriented parser for the AI Change Language (ACL) DSL.
pub mod parser;

#[cfg(feature = "storage-bridge")]
pub mod storage_bridge;
