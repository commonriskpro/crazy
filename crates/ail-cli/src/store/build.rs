use crate::error::CliError;

use super::{StoreHandle, file::file_handle, memory::memory_handle, postgres::connect_postgres};

// build_store ---------------------------------------------------------------

/// Construct the appropriate `StoreHandle` from an optional database URL.
///
/// Resolution order:
/// 1. `db_url` argument (from `--database-url` flag).
/// 2. `AIL_DATABASE_URL` environment variable.
/// 3. Local file store if `.ail/` exists in the current directory.
/// 4. In-memory fallback.
///
/// # Errors
///
/// Returns `Err(CliError::Storage(_))` if a DB URL is provided but the
/// connection fails.
pub async fn build_store(db_url: Option<&str>) -> Result<StoreHandle, CliError> {
    // 1. Explicit flag.
    if let Some(url) = db_url {
        return connect_postgres(url).await;
    }
    // 2. Environment variable.
    if let Ok(url) = std::env::var("AIL_DATABASE_URL") {
        return connect_postgres(&url).await;
    }
    // 3. Local file store when the project has been initialized.
    let ail_dir = std::env::current_dir()?.join(".ail");
    if ail_dir.join("HEAD").exists() && ail_dir.join("store").join("objects").exists() {
        return Ok(file_handle(ail_dir));
    }
    // 4. In-memory fallback.
    Ok(memory_handle())
}
