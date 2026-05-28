mod pool;

pub mod migrations;

pub use pool::{create_pool, create_read_pool, create_write_pool};

use sqlx::Sqlite;
use sqlx_migrator::error::Error;
use sqlx_migrator::migrator::{Info as _, Migrator};

/// Combined migrator: evento's event-store schema first, then the
/// projection tables owned by domain crates.
pub fn build_migrator() -> Result<Migrator<Sqlite>, Error> {
    // Start from evento's preconfigured migrator (event / subscriber tables).
    let mut migrator = evento::sql_migrator::new::<Sqlite>()?;
    migrator.add_migrations(migrations::migrations())?;
    Ok(migrator)
}
