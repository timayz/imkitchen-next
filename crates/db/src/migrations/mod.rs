use sqlx::Sqlite;
use sqlx_migrator::migration::Migration;

pub fn migrations() -> Vec<Box<dyn Migration<Sqlite>>> {
    vec![]
}
