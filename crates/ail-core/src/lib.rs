/// Crate version, sourced directly from `Cargo.toml` at compile time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod graph_index;
pub mod semantic_graph;

pub use graph_index::GraphIndex;
pub use semantic_graph::{BlockRef, ContractRef, EffectRef, ProofObligationRef, RuntimeCheckRef};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_non_empty() {
        assert!(!VERSION.is_empty());
    }
}
