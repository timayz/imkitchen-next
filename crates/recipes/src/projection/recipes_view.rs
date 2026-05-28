//! `recipes_view` — the recipe catalog read model.
//!
//! One row per recipe. Both `/recipes` (index, list view) and `/recipes/{id}`
//! (detail) are served from this table. The list view ignores
//! `ingredients_json` / `steps_json` / `description`; the detail view uses
//! them all.
//!
//! Filtering + search match the in-memory `fake_recipes()` behaviour the
//! handler currently relies on:
//!   - `filter=main|side|…` matches `meal_type`.
//!   - `q=foo` matches `title` or `cuisine` case-insensitively.
//!
//! Tags/ingredients/steps are stored as JSON for simplicity — these are
//! display-only and never queried. If filtering by tag becomes a thing we
//! split into a join table.

use anyhow::Result;
use evento::metadata::Event;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::recipe::{
    IngredientFact, IngredientsReplaced, RecipeDeleted, RecipeDrafted, RecipeRecategorized,
    RecipeRedescribed, RecipeRenamed, RecipeRetagged, RecipeRetimed, StepFact, StepsReplaced,
    Unit,
};

/// One row from `recipes_view`. Field names mirror the
/// `web/server/src/recipes.rs::Recipe` view struct so the handler can pass
/// rows straight to the template.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RecipeRow {
    pub id: String,
    pub owner_id: String,
    pub title: String,
    pub meal_type: String,
    pub emoji: String,
    pub cuisine: String,
    pub time_minutes: i64,
    pub servings: i64,
    pub rating: f64,
    pub difficulty: String,
    pub description: String,
    /// JSON `["Vegan", "Gluten-free"]`
    pub tags_json: String,
    /// JSON `[{"name":"…","quantity":250,"unit":"g"}, …]`. `quantity` is
    /// `null` for vague rows; `unit` is one of the [`Unit`] slugs.
    pub ingredients_json: String,
    /// JSON `[{"text":"…","wait_minutes":5}, …]`
    pub steps_json: String,
    pub created_at: i64,
}

impl RecipeRow {
    pub fn tags(&self) -> Vec<String> {
        serde_json::from_str(&self.tags_json).unwrap_or_default()
    }

    pub fn ingredients(&self) -> Vec<IngredientFact> {
        // Use the serializable view because `IngredientFact` derives bitcode,
        // not serde — we round-trip via this intermediate.
        let view: Vec<IngredientView> =
            serde_json::from_str(&self.ingredients_json).unwrap_or_default();
        view.into_iter()
            .map(|v| IngredientFact {
                name: v.name,
                quantity: v.quantity,
                unit: v.unit,
            })
            .collect()
    }

    pub fn steps(&self) -> Vec<StepFact> {
        let view: Vec<StepView> = serde_json::from_str(&self.steps_json).unwrap_or_default();
        view.into_iter()
            .map(|v| StepFact {
                wait_minutes: v.wait_minutes,
                text: v.text,
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IngredientView {
    name: String,
    #[serde(default)]
    quantity: Option<f32>,
    #[serde(default)]
    unit: Unit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StepView {
    #[serde(alias = "minutes")]
    wait_minutes: u32,
    text: String,
}

/// Filter for [`list_for_owner`]. `None` = no filter (the "all" chip).
#[derive(Debug, Default, Clone)]
pub struct RecipesQuery {
    pub meal_type: Option<String>,
    pub search: Option<String>,
}

/// Count of all recipes owned by `owner_id`. The index header shows this as
/// "Library · {N} recipes" — it's the user's total, not the filtered total.
pub async fn total_count(pool: &SqlitePool, owner_id: &str) -> Result<i64> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM recipes_view WHERE owner_id = ?")
        .bind(owner_id)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

pub async fn list_for_owner(
    pool: &SqlitePool,
    owner_id: &str,
    query: &RecipesQuery,
) -> Result<Vec<RecipeRow>> {
    // Hand-built SQL because sqlx's compile-time check needs a DATABASE_URL
    // at build time and we're avoiding that infra for now. The two optional
    // clauses are mutually exclusive in shape so we branch.
    let needle = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{}%", s.to_lowercase()));

    let rows = match (query.meal_type.as_deref(), needle.as_deref()) {
        (None, None) => {
            sqlx::query_as::<_, RecipeRow>(
                "SELECT id, owner_id, title, meal_type, emoji, cuisine, time_minutes, \
                        servings, rating, difficulty, description, tags_json, \
                        ingredients_json, steps_json, created_at \
                 FROM recipes_view \
                 WHERE owner_id = ? \
                 ORDER BY created_at DESC, id DESC",
            )
            .bind(owner_id)
            .fetch_all(pool)
            .await?
        }
        (Some(meal_type), None) => {
            sqlx::query_as::<_, RecipeRow>(
                "SELECT id, owner_id, title, meal_type, emoji, cuisine, time_minutes, \
                        servings, rating, difficulty, description, tags_json, \
                        ingredients_json, steps_json, created_at \
                 FROM recipes_view \
                 WHERE owner_id = ? AND meal_type = ? \
                 ORDER BY created_at DESC, id DESC",
            )
            .bind(owner_id)
            .bind(meal_type)
            .fetch_all(pool)
            .await?
        }
        (None, Some(needle)) => {
            sqlx::query_as::<_, RecipeRow>(
                "SELECT id, owner_id, title, meal_type, emoji, cuisine, time_minutes, \
                        servings, rating, difficulty, description, tags_json, \
                        ingredients_json, steps_json, created_at \
                 FROM recipes_view \
                 WHERE owner_id = ? \
                   AND (LOWER(title) LIKE ?1 OR LOWER(cuisine) LIKE ?1) \
                 ORDER BY created_at DESC, id DESC",
            )
            .bind(owner_id)
            .bind(needle)
            .fetch_all(pool)
            .await?
        }
        (Some(meal_type), Some(needle)) => {
            sqlx::query_as::<_, RecipeRow>(
                "SELECT id, owner_id, title, meal_type, emoji, cuisine, time_minutes, \
                        servings, rating, difficulty, description, tags_json, \
                        ingredients_json, steps_json, created_at \
                 FROM recipes_view \
                 WHERE owner_id = ? AND meal_type = ? \
                   AND (LOWER(title) LIKE ?1 OR LOWER(cuisine) LIKE ?1) \
                 ORDER BY created_at DESC, id DESC",
            )
            .bind(owner_id)
            .bind(meal_type)
            .bind(needle)
            .fetch_all(pool)
            .await?
        }
    };

    Ok(rows)
}

pub async fn find_for_owner(
    pool: &SqlitePool,
    owner_id: &str,
    id: &str,
) -> Result<Option<RecipeRow>> {
    let row = sqlx::query_as::<_, RecipeRow>(
        "SELECT id, owner_id, title, meal_type, emoji, cuisine, time_minutes, \
                servings, rating, difficulty, description, tags_json, \
                ingredients_json, steps_json, created_at \
         FROM recipes_view \
         WHERE owner_id = ? AND id = ?",
    )
    .bind(owner_id)
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Subscription handler — upsert a row on `RecipeDrafted`. Idempotent via
/// `ON CONFLICT DO UPDATE`: if the event is redelivered the row converges to
/// the same state.
pub async fn apply_drafted(
    pool: &SqlitePool,
    event: &Event<RecipeDrafted>,
) -> Result<()> {
    let time_minutes = (event.data.prep_minutes + event.data.cook_minutes) as i64;
    let tags_json = serde_json::to_string(&event.data.tags)?;
    let ingredient_view: Vec<IngredientView> = event
        .data
        .ingredients
        .iter()
        .map(|i| IngredientView {
            name: i.name.clone(),
            quantity: i.quantity,
            unit: i.unit,
        })
        .collect();
    let step_view: Vec<StepView> = event
        .data
        .steps
        .iter()
        .map(|s| StepView {
            wait_minutes: s.wait_minutes,
            text: s.text.clone(),
        })
        .collect();
    let ingredients_json = serde_json::to_string(&ingredient_view)?;
    let steps_json = serde_json::to_string(&step_view)?;

    sqlx::query(
        "INSERT INTO recipes_view (\
            id, owner_id, title, meal_type, emoji, cuisine, time_minutes, \
            servings, rating, difficulty, description, tags_json, \
            ingredients_json, steps_json, created_at \
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(id) DO UPDATE SET \
            title = excluded.title, \
            meal_type = excluded.meal_type, \
            emoji = excluded.emoji, \
            cuisine = excluded.cuisine, \
            time_minutes = excluded.time_minutes, \
            servings = excluded.servings, \
            difficulty = excluded.difficulty, \
            description = excluded.description, \
            tags_json = excluded.tags_json, \
            ingredients_json = excluded.ingredients_json, \
            steps_json = excluded.steps_json",
    )
    .bind(&event.aggregator_id)
    .bind(&event.data.owner_id)
    .bind(&event.data.title)
    .bind(&event.data.meal_type)
    .bind(&event.data.emoji)
    .bind(&event.data.cuisine)
    .bind(time_minutes)
    .bind(event.data.servings as i64)
    // Rating starts at 0 — we have no review machinery yet.
    .bind(0.0_f64)
    .bind(&event.data.difficulty)
    .bind(&event.data.description)
    .bind(tags_json)
    .bind(ingredients_json)
    .bind(steps_json)
    .bind(event.timestamp as i64)
    .execute(pool)
    .await?;

    Ok(())
}

// ── Edit handlers ──────────────────────────────────────────────────────
//
// One narrow UPDATE per event. All are idempotent (replaying the event
// converges to the same row state), and they target a specific recipe by
// id so they're safe to register on the existing recipes_view subscription.

pub async fn apply_renamed(pool: &SqlitePool, event: &Event<RecipeRenamed>) -> Result<()> {
    sqlx::query("UPDATE recipes_view SET title = ? WHERE id = ?")
        .bind(&event.data.new_title)
        .bind(&event.aggregator_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn apply_recategorized(
    pool: &SqlitePool,
    event: &Event<RecipeRecategorized>,
) -> Result<()> {
    sqlx::query(
        "UPDATE recipes_view \
         SET meal_type = ?, cuisine = ?, emoji = ? \
         WHERE id = ?",
    )
    .bind(&event.data.meal_type)
    .bind(&event.data.cuisine)
    .bind(&event.data.emoji)
    .bind(&event.aggregator_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn apply_retimed(pool: &SqlitePool, event: &Event<RecipeRetimed>) -> Result<()> {
    // Recompute `time_minutes` from `prep + cook` to keep parity with
    // `apply_drafted`. The projection only stores the sum (which is what
    // every template displays); prep/cook are only carried on the events.
    let time_minutes = (event.data.prep_minutes + event.data.cook_minutes) as i64;
    sqlx::query(
        "UPDATE recipes_view \
         SET time_minutes = ?, servings = ?, difficulty = ? \
         WHERE id = ?",
    )
    .bind(time_minutes)
    .bind(event.data.servings as i64)
    .bind(&event.data.difficulty)
    .bind(&event.aggregator_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn apply_redescribed(
    pool: &SqlitePool,
    event: &Event<RecipeRedescribed>,
) -> Result<()> {
    sqlx::query("UPDATE recipes_view SET description = ? WHERE id = ?")
        .bind(&event.data.description)
        .bind(&event.aggregator_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn apply_retagged(pool: &SqlitePool, event: &Event<RecipeRetagged>) -> Result<()> {
    let tags_json = serde_json::to_string(&event.data.tags)?;
    sqlx::query("UPDATE recipes_view SET tags_json = ? WHERE id = ?")
        .bind(tags_json)
        .bind(&event.aggregator_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn apply_ingredients_replaced(
    pool: &SqlitePool,
    event: &Event<IngredientsReplaced>,
) -> Result<()> {
    let view: Vec<IngredientView> = event
        .data
        .ingredients
        .iter()
        .map(|i| IngredientView {
            name: i.name.clone(),
            quantity: i.quantity,
            unit: i.unit,
        })
        .collect();
    let ingredients_json = serde_json::to_string(&view)?;
    sqlx::query("UPDATE recipes_view SET ingredients_json = ? WHERE id = ?")
        .bind(ingredients_json)
        .bind(&event.aggregator_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn apply_steps_replaced(
    pool: &SqlitePool,
    event: &Event<StepsReplaced>,
) -> Result<()> {
    let view: Vec<StepView> = event
        .data
        .steps
        .iter()
        .map(|s| StepView {
            wait_minutes: s.wait_minutes,
            text: s.text.clone(),
        })
        .collect();
    let steps_json = serde_json::to_string(&view)?;
    sqlx::query("UPDATE recipes_view SET steps_json = ? WHERE id = ?")
        .bind(steps_json)
        .bind(&event.aggregator_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// `RecipeDeleted` hard-deletes the projection row. Replaying the event
/// after the row is already gone is a no-op (zero rows affected).
pub async fn apply_deleted(pool: &SqlitePool, event: &Event<RecipeDeleted>) -> Result<()> {
    sqlx::query("DELETE FROM recipes_view WHERE id = ?")
        .bind(&event.aggregator_id)
        .execute(pool)
        .await?;
    Ok(())
}
