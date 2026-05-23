mod pool;

pub mod migrations;

pub use pool::{create_pool, create_read_pool, create_write_pool};

use sqlx::Sqlite;
use sqlx_migrator::error::Error;
use sqlx_migrator::migrator::{Info as _, Migrator};

pub fn build_migrator() -> Result<Migrator<Sqlite>, Error> {
    let mut migrator = Migrator::<Sqlite>::default();
    migrator.add_migrations(migrations::migrations())?;
    Ok(migrator)
}
