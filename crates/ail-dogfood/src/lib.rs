// ── ail-dogfood ───────────────────────────────────────────────────────────
//
// Leaf dogfooding crate: demonstrates and validates the AIL toolchain by
// building self-referential models using its own types.
//
// No other crate depends on `ail-dogfood`.  All builders are pure Rust
// functions returning in-memory values; no file I/O, no side effects.
//
// # Modules
//
// | Module | Contents |
// |--------|----------|
// | `graph_self_model` | `build_graph_self_model()` — constructs a `SemanticGraph` whose nodes describe the toolchain's own core types |
// | `changeset_self` | `build_changeset_self()` — constructs a self-referential `ChangeSet` |
// | `stdlib_projection` | `project_stdlib_to_graph()` — projects the v1 stdlib registry into a `SemanticGraph` |

/// Builder for a `SemanticGraph` that models the toolchain's own core types.
pub mod graph_self_model;

/// Builder for a self-referential `ChangeSet` describing itself.
pub mod changeset_self;

/// Stable metadata contracts for dogfood fixtures and example programs.
pub mod fixture_contracts;

/// Projects the stdlib registry into a `SemanticGraph`.
pub mod stdlib_projection;
