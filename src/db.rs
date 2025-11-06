use sqlx::{postgres::PgPoolOptions, PgPool};
use std::time::Duration;

pub async fn init_pg_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await
}