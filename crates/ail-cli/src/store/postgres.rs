use ail_storage::PostgresGraphStore;

use crate::error::CliError;

use super::StoreHandle;

pub(super) async fn connect_postgres(url: &str) -> Result<StoreHandle, CliError> {
    let store = PostgresGraphStore::connect(url).await?;
    Ok(StoreHandle::Postgres(store))
}
