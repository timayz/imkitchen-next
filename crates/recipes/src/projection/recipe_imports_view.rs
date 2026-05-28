//! `recipe_imports_view` — one row per import job.
//!
//! Feeds the three stages of `/recipes/import`:
//!
//! - `stage = started`   → the upload screen is still showing (waiting for the
//!   parser to emit `ImportPreviewed`).
//! - `stage = previewed` → review screen (candidates are populated).
//! - `stage = completed` → done screen (`imported_recipe_ids` populated).
//! - `stage = failed`    → terminal failure — reason set, no recipes.
//!
//! The done-stage stats (Imported / Skipped / Duplicate) are derived at
//! query time from the count of `imported_recipe_ids` vs picked count. We
//! don't materialize those into columns — they're cheap to compute.

use anyhow::Result;
use evento::metadata::Event;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::import::{
    ImportCompleted, ImportConfirmed, ImportFailed, ImportPreviewed, ImportStarted,
    ParsedCandidate,
};

/// One import row. Fields map onto what `recipes/_import_preview.html` and
/// `_import_done.html` need.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ImportRow {
    pub id: String,
    pub owner_id: String,
    pub source: String,
    pub source_label: String,
    pub stage: String,
    /// JSON `[ParsedCandidate, …]`. Empty `[]` until `ImportPreviewed` arrives.
    pub candidates_json: String,
    /// JSON `[picked_id, …]`. Empty until confirmation.
    pub picked_json: String,
    /// JSON `[recipe_id, …]`. Empty until the saga finishes.
    pub recipe_ids_json: String,
    /// Empty unless the saga ended in failure.
    pub failure_reason: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl ImportRow {
    pub fn candidates(&self) -> Vec<ParsedCandidateView> {
        serde_json::from_str(&self.candidates_json).unwrap_or_default()
    }
    pub fn picked_ids(&self) -> Vec<String> {
        serde_json::from_str(&self.picked_json).unwrap_or_default()
    }
    pub fn recipe_ids(&self) -> Vec<String> {
        serde_json::from_str(&self.recipe_ids_json).unwrap_or_default()
    }
}

/// Serializable mirror of [`ParsedCandidate`] — `bitcode` derives aren't
/// `serde`-compatible.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParsedCandidateView {
    pub id: String,
    pub title: String,
    pub emoji: String,
    pub meal_type: String,
    pub ingredient_count: u32,
    pub step_count: u32,
    pub warn: Option<String>,
    pub broken: bool,
    pub selected: bool,
}

impl From<&ParsedCandidate> for ParsedCandidateView {
    fn from(c: &ParsedCandidate) -> Self {
        Self {
            id: c.id.clone(),
            title: c.title.clone(),
            emoji: c.emoji.clone(),
            meal_type: c.meal_type.clone(),
            ingredient_count: c.ingredient_count,
            step_count: c.step_count,
            warn: c.warn.clone(),
            broken: c.broken,
            selected: c.selected,
        }
    }
}

pub async fn find(pool: &SqlitePool, id: &str) -> Result<Option<ImportRow>> {
    let row = sqlx::query_as::<_, ImportRow>(
        "SELECT id, owner_id, source, source_label, stage, candidates_json, \
                picked_json, recipe_ids_json, failure_reason, created_at, updated_at \
         FROM recipe_imports_view WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Latest import for this owner — the import index page redirects users to
/// the in-progress job without making them paste an id.
pub async fn latest_for_owner(pool: &SqlitePool, owner_id: &str) -> Result<Option<ImportRow>> {
    let row = sqlx::query_as::<_, ImportRow>(
        "SELECT id, owner_id, source, source_label, stage, candidates_json, \
                picked_json, recipe_ids_json, failure_reason, created_at, updated_at \
         FROM recipe_imports_view \
         WHERE owner_id = ? \
         ORDER BY updated_at DESC, id DESC LIMIT 1",
    )
    .bind(owner_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn apply_started(pool: &SqlitePool, event: &Event<ImportStarted>) -> Result<()> {
    sqlx::query(
        "INSERT INTO recipe_imports_view (\
            id, owner_id, source, source_label, stage, candidates_json, \
            picked_json, recipe_ids_json, failure_reason, created_at, updated_at \
         ) VALUES (?, ?, ?, ?, 'started', '[]', '[]', '[]', '', ?, ?) \
         ON CONFLICT(id) DO UPDATE SET \
            owner_id = excluded.owner_id, \
            source = excluded.source, \
            source_label = excluded.source_label, \
            stage = 'started'",
    )
    .bind(&event.aggregator_id)
    .bind(&event.data.owner_id)
    .bind(&event.data.source)
    .bind(&event.data.source_label)
    .bind(event.timestamp as i64)
    .bind(event.timestamp as i64)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn apply_previewed(pool: &SqlitePool, event: &Event<ImportPreviewed>) -> Result<()> {
    let view: Vec<ParsedCandidateView> = event
        .data
        .candidates
        .iter()
        .map(ParsedCandidateView::from)
        .collect();
    let candidates_json = serde_json::to_string(&view)?;
    sqlx::query(
        "UPDATE recipe_imports_view \
         SET candidates_json = ?, stage = 'previewed', updated_at = ? \
         WHERE id = ?",
    )
    .bind(candidates_json)
    .bind(event.timestamp as i64)
    .bind(&event.aggregator_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn apply_confirmed(pool: &SqlitePool, event: &Event<ImportConfirmed>) -> Result<()> {
    let picked_json = serde_json::to_string(&event.data.picked_ids)?;
    sqlx::query(
        "UPDATE recipe_imports_view \
         SET picked_json = ?, updated_at = ? \
         WHERE id = ?",
    )
    .bind(picked_json)
    .bind(event.timestamp as i64)
    .bind(&event.aggregator_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn apply_completed(pool: &SqlitePool, event: &Event<ImportCompleted>) -> Result<()> {
    let recipe_ids_json = serde_json::to_string(&event.data.recipe_ids)?;
    sqlx::query(
        "UPDATE recipe_imports_view \
         SET recipe_ids_json = ?, stage = 'completed', updated_at = ? \
         WHERE id = ?",
    )
    .bind(recipe_ids_json)
    .bind(event.timestamp as i64)
    .bind(&event.aggregator_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn apply_failed(pool: &SqlitePool, event: &Event<ImportFailed>) -> Result<()> {
    sqlx::query(
        "UPDATE recipe_imports_view \
         SET stage = 'failed', failure_reason = ?, updated_at = ? \
         WHERE id = ?",
    )
    .bind(&event.data.reason)
    .bind(event.timestamp as i64)
    .bind(&event.aggregator_id)
    .execute(pool)
    .await?;
    Ok(())
}
