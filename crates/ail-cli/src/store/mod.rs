// ail-cli::store ------------------------------------------------------------
//
// `StoreHandle` abstracts over the supported storage backends:
//
//   * `Memory` - in-process `ObjectBackedGraphStore<MemoryObjectStore>`.
//     Data is lost when the process exits. Used when no `--database-url`
//     or `AIL_DATABASE_URL` is configured.
//
//   * `File` - local `.ail/` object store backed by content-addressed files.
//     Used when `.ail/` exists in the current working directory.
//
//   * `Postgres` - durable `PostgresGraphStore` backed by a Postgres database.
//     Data persists across invocations. Used when a DB URL is configured.
//
// `build_store` constructs the appropriate variant from the optional URL and
// is the sole entry-point for store creation in the CLI.
//
// File-store implementation details live in `store_file`.
// Doctor / GC report types and logic live in `store_doctor`.

mod build;
mod file;
mod handle;
mod memory;
mod postgres;
#[cfg(test)]
mod tests;

pub use build::build_store;
pub use file::file_store;
pub use handle::StoreHandle;

// Re-exports (keep `crate::store::X` stable for all callers) ----------------
pub use crate::store_doctor::{doctor, gc};
#[cfg(test)]
pub use crate::store_file::init_file_layout;
pub use crate::store_file::{FileObjectStore, init_file_layout_with_branch};
pub(crate) use crate::store_file::{atomic_write, is_object_file_name};

#[cfg(test)]
pub use memory::memory_store;

#[cfg(test)]
use postgres::connect_postgres;
