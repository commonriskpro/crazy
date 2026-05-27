// ── ail-context::dto ──────────────────────────────────────────────────────
//
// Query/response/selector data-transfer objects for the context API.
//
// # Determinism contract
//
// All DTOs use `Vec` and `BTreeMap` only — never `HashMap` — to satisfy
// the CBOR determinism contract inherited from `ail-core` and `ail-storage`.
// No floating-point values; timestamps are `u64` Unix milliseconds.

mod budget;
mod provenance;
mod query;
mod redaction;
mod response;

pub use budget::QueryBudget;
pub use provenance::{CONTEXT_SCHEMA_V1, IndexInfo, ProvenanceBlock};
pub use query::{ContextQuery, QueryScope, SnapshotSelector};
pub use redaction::{RedactionPolicy, RedactionState};
pub use response::{
    ContextResponse, FreshnessStatus, ImpactInfo, RefactorInfo, RepairOption, ResponseLimits,
};

#[cfg(test)]
mod tests;
