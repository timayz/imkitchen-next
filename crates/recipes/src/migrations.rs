//! Read-side migrations for the recipes context.
//!
//! Two tables:
//! - `recipes_view` — recipe catalog + detail.
//! - `recipe_imports_view` — one row per import job.
//!
//! These are *projection* tables — they are not the source of truth. The
//! event log is. A future `imkitchen reset-projections` command should drop
//! and re-apply just these migrations, then replay every recipe event.

use sqlx::Sqlite;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::vec_box;

/// All read-side migrations for the recipes context. Append to this list
/// (never insert in the middle) when adding new projection tables.
pub fn migrations() -> Vec<Box<dyn Migration<Sqlite>>> {
    vec_box![CreateRecipesView, CreateRecipeImportsView]
}

pub struct CreateRecipesView;

sqlx_migrator::sqlite_migration!(
    CreateRecipesView,
    "imkitchen-recipes",
    "0001_create_recipes_view",
    vec_box![],
    vec_box![(
        "CREATE TABLE IF NOT EXISTS recipes_view (\n\
            id TEXT NOT NULL PRIMARY KEY,\n\
            owner_id TEXT NOT NULL,\n\
            title TEXT NOT NULL,\n\
            meal_type TEXT NOT NULL,\n\
            emoji TEXT NOT NULL DEFAULT '',\n\
            cuisine TEXT NOT NULL DEFAULT '',\n\
            time_minutes INTEGER NOT NULL DEFAULT 0,\n\
            servings INTEGER NOT NULL DEFAULT 1,\n\
            rating REAL NOT NULL DEFAULT 0.0,\n\
            difficulty TEXT NOT NULL DEFAULT '',\n\
            description TEXT NOT NULL DEFAULT '',\n\
            tags_json TEXT NOT NULL DEFAULT '[]',\n\
            ingredients_json TEXT NOT NULL DEFAULT '[]',\n\
            steps_json TEXT NOT NULL DEFAULT '[]',\n\
            created_at INTEGER NOT NULL DEFAULT 0\n\
        );\n\
        CREATE INDEX IF NOT EXISTS idx_recipes_view_owner_created \n\
            ON recipes_view (owner_id, created_at DESC);\n\
        CREATE INDEX IF NOT EXISTS idx_recipes_view_owner_meal \n\
            ON recipes_view (owner_id, meal_type);",
        "DROP TABLE IF EXISTS recipes_view;",
    )]
);

pub struct CreateRecipeImportsView;

sqlx_migrator::sqlite_migration!(
    CreateRecipeImportsView,
    "imkitchen-recipes",
    "0002_create_recipe_imports_view",
    vec_box![CreateRecipesView],
    vec_box![(
        "CREATE TABLE IF NOT EXISTS recipe_imports_view (\n\
            id TEXT NOT NULL PRIMARY KEY,\n\
            owner_id TEXT NOT NULL,\n\
            source TEXT NOT NULL,\n\
            source_label TEXT NOT NULL DEFAULT '',\n\
            stage TEXT NOT NULL DEFAULT 'started',\n\
            candidates_json TEXT NOT NULL DEFAULT '[]',\n\
            picked_json TEXT NOT NULL DEFAULT '[]',\n\
            recipe_ids_json TEXT NOT NULL DEFAULT '[]',\n\
            failure_reason TEXT NOT NULL DEFAULT '',\n\
            created_at INTEGER NOT NULL DEFAULT 0,\n\
            updated_at INTEGER NOT NULL DEFAULT 0\n\
        );\n\
        CREATE INDEX IF NOT EXISTS idx_recipe_imports_view_owner_updated \n\
            ON recipe_imports_view (owner_id, updated_at DESC);",
        "DROP TABLE IF EXISTS recipe_imports_view;",
    )]
);
