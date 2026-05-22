// ── ail-change::acl_migrator ─────────────────────────────────────────────
//
// Version-aware ACL canonicalization migrators.
//
// # Design
//
// Each concrete migrator handles exactly one version step (source → target).
// `run_migration_chain` drives the chain: starting from the document's
// declared `acl_version`, it repeatedly finds and applies the migrator whose
// `source_version` matches the current version, advancing until the version
// matches `CURRENT_ACL_VERSION`.
//
// # Registered chain (oldest → newest)
//
//   0.9 → 1.0  via `AclMigratorV0_9ToV1_0`   (renames deprecated short forms)
//   1.0 → 1.1  via `AclMigrator_1_0_to_1_1`   (prepared; not yet active)
//
// When CURRENT_ACL_VERSION is bumped from "1.0" to "1.1", the second
// migrator automatically becomes part of the live chain.

use crate::parser::ParsedChangeSet;

// ── Current version ───────────────────────────────────────────────────────

/// The ACL language version this build of `ail-change` targets.
///
/// `canonicalize_parsed` / `try_canonicalize_parsed` will run the migrator
/// chain when the document declares an older version, normalising it to
/// this version before canonicalization proceeds.
pub const CURRENT_ACL_VERSION: &str = "1.0";

// ── MigrateError ─────────────────────────────────────────────────────────

/// Errors that can occur during ACL version migration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MigrateError {
    /// The document declares a version for which no migration path exists.
    UnknownVersion(String),
    /// A migrator returned an internal failure.
    MigrationFailed {
        from: String,
        to: String,
        reason: String,
    },
}

impl std::fmt::Display for MigrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrateError::UnknownVersion(v) => write!(f, "unknown ACL version: {v}"),
            MigrateError::MigrationFailed { from, to, reason } => {
                write!(f, "migration {from} → {to} failed: {reason}")
            }
        }
    }
}

impl std::error::Error for MigrateError {}

// ── AclMigrator trait ─────────────────────────────────────────────────────

/// Version-aware canonicalization migrator.
///
/// Each implementation handles exactly one version step (`from` → `to`).
/// The runtime runs a chain of migrators until the document version matches
/// [`CURRENT_ACL_VERSION`].
pub trait AclMigrator: Send + Sync {
    /// Apply in-place migrations to bring `changeset` from `from` to `to`.
    ///
    /// Implementations rename deprecated op verbs and normalise field names
    /// as required to conform to the target version's semantics.
    fn migrate(
        &self,
        changeset: &mut ParsedChangeSet,
        from: &str,
        to: &str,
    ) -> Result<(), MigrateError>;
}

// ── Concrete migrators ────────────────────────────────────────────────────

/// Migrates a changeset from ACL v0.9 to v1.0.
///
/// Changes applied:
/// - Renames deprecated verb `create_fn`  → `create_function`.
/// - Renames deprecated verb `add_field`  → `add_param`.
/// - Normalises arg key `typ`             → `type` (if present).
pub struct AclMigratorV0_9ToV1_0;

impl AclMigrator for AclMigratorV0_9ToV1_0 {
    fn migrate(
        &self,
        changeset: &mut ParsedChangeSet,
        _from: &str,
        _to: &str,
    ) -> Result<(), MigrateError> {
        for op in &mut changeset.parsed_ops {
            match op.verb.as_str() {
                "create_fn" => op.verb = "create_function".to_string(),
                "add_field" => op.verb = "add_param".to_string(),
                _ => {}
            }
            // Normalise arg key: "typ" → "type" (preserves explicit "type" if both present).
            if let Some(val) = op.args.remove("typ") {
                op.args.entry("type".to_string()).or_insert(val);
            }
        }
        Ok(())
    }
}

/// Migrates a changeset from ACL v1.0 to v1.1.
///
/// Registered in the chain but not yet active (CURRENT_ACL_VERSION = "1.0").
/// When CURRENT is bumped to "1.1" this migrator becomes the live final step.
///
/// Changes applied (preparatory):
/// - Renames deprecated verb `set_field`      → `set_return`.
/// - Normalises arg key `return_type`         → `type` for set-family ops.
#[allow(non_camel_case_types)]
pub struct AclMigrator_1_0_to_1_1;

impl AclMigrator for AclMigrator_1_0_to_1_1 {
    fn migrate(
        &self,
        changeset: &mut ParsedChangeSet,
        _from: &str,
        _to: &str,
    ) -> Result<(), MigrateError> {
        for op in &mut changeset.parsed_ops {
            if op.verb == "set_field" {
                op.verb = "set_return".to_string();
            }
            // Normalise "return_type" → "type" in args.
            if let Some(val) = op.args.remove("return_type") {
                op.args.entry("type".to_string()).or_insert(val);
            }
        }
        Ok(())
    }
}

// ── Migration chain ───────────────────────────────────────────────────────

/// A single entry in the migrator chain: (source_version, target_version, migrator).
type MigratorEntry = (&'static str, &'static str, Box<dyn AclMigrator>);

/// Build the full migrator chain in oldest-to-newest order.
fn default_migrator_chain() -> Vec<MigratorEntry> {
    vec![
        ("0.9", "1.0", Box::new(AclMigratorV0_9ToV1_0)),
        ("1.0", "1.1", Box::new(AclMigrator_1_0_to_1_1)),
    ]
}

// ── run_migration_chain ───────────────────────────────────────────────────

/// Run the migrator chain to bring `pcs.acl_version` up to `target`.
///
/// Starting from `pcs.acl_version`, repeatedly finds and applies the
/// registered migrator whose source version matches.  Returns `Ok(pcs)` with
/// `pcs.acl_version == target` on success.
///
/// # Errors
///
/// - [`MigrateError::UnknownVersion`] — no migrator is registered for the
///   current version step (e.g. a future or completely unknown version).
/// - [`MigrateError::MigrationFailed`] — a migrator returned an internal error.
pub fn run_migration_chain(
    mut pcs: ParsedChangeSet,
    target: &str,
) -> Result<ParsedChangeSet, MigrateError> {
    if pcs.acl_version == target {
        return Ok(pcs);
    }
    let chain = default_migrator_chain();
    let mut current = pcs.acl_version.clone();

    while current != target {
        let entry = chain
            .iter()
            .find(|(src, _, _)| *src == current.as_str())
            .ok_or_else(|| MigrateError::UnknownVersion(current.clone()))?;

        let (from, to, migrator) = entry;
        migrator.migrate(&mut pcs, from, to)?;
        current = to.to_string();
        pcs.acl_version = current.clone();
    }

    Ok(pcs)
}
