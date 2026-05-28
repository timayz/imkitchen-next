use sqlx::Sqlite;
use sqlx_migrator::migration::Migration;

pub fn migrations() -> Vec<Box<dyn Migration<Sqlite>>> {
    let mut out: Vec<Box<dyn Migration<Sqlite>>> = Vec::new();
    out.extend(imkitchen_recipes::migrations::migrations());
    out
}
