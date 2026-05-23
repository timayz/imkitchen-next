use std::str::FromStr;
use std::time::Duration;

use sqlx::ConnectOptions;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use tracing::log::LevelFilter;

fn base_options(database_url: &str) -> Result<SqliteConnectOptions, sqlx::Error> {
    Ok(SqliteConnectOptions::from_str(database_url)?
        .busy_timeout(Duration::from_millis(5000))
        .foreign_keys(true)
        .pragma("cache_size", "-20000")
        .pragma("temp_store", "memory")
        .log_statements(LevelFilter::Debug))
}

pub async fn create_read_pool(
    database_url: &str,
    max_connections: u32,
) -> Result<SqlitePool, sqlx::Error> {
    let options = base_options(database_url)?.read_only(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections)
        .connect_with(options)
        .await?;

    tracing::info!(
        "Created read-only pool with {} max connections",
        max_connections
    );
    Ok(pool)
}

pub async fn create_write_pool(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let options = base_options(database_url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;

    tracing::info!("Created read-write pool with 1 max connection");
    Ok(pool)
}

pub async fn create_pool(
    database_url: &str,
    max_connections: u32,
) -> Result<SqlitePool, sqlx::Error> {
    let options = base_options(database_url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal);

    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections)
        .connect_with(options)
        .await?;

    tracing::info!("Created pool with {} max connections", max_connections);
    Ok(pool)
}
