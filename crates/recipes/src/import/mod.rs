//! Import flow — `RecipeImport` aggregate + process manager (saga).
//!
//! The import flow spans the user's import "session" (one `RecipeImport`
//! aggregate) and one or more `Recipe` aggregates (one per imported recipe).
//! That cross-aggregate span is what makes it a saga; we model the saga's
//! state explicitly as its own aggregate so failures have a place to land
//! (`ImportFailed`) and the UI has a single id to look up.
//!
//! State transitions:
//!
//! ```text
//!   Started ── ImportPreviewed ──► Previewed
//!                                       │
//!                                       │ ImportConfirmed
//!                                       ▼
//!                                  Confirming ── per-recipe RecipeDrafted ──► Completed
//!                                       │
//!                                       └─ any failure ──────────────────────► Failed
//! ```
//!
//! Compensating actions:
//!
//! - A parser failure between `Started` and `Previewed` ends the saga with
//!   `ImportFailed { reason }`. No recipes were drafted yet → nothing to
//!   compensate; the user sees the failure on the upload screen.
//! - A failure midway through drafting (e.g. one of the chosen candidates
//!   would violate a `Recipe` invariant) marks `ImportFailed` *but leaves
//!   already-drafted recipes in place* — they're real, finished facts. The
//!   read-side `recipe_imports_view` exposes both `imported_recipe_ids` and
//!   the failure reason so the UI can surface "we got 3 of 5; here's the one
//!   that broke". We do NOT roll back drafted recipes; the user explicitly
//!   asked for those and can delete them in a later iteration.
//!
//! All external effects (URL fetch, OCR, parser) live behind the
//! [`parser::RecipeParser`] trait. The default impl is a no-op that returns
//! a fixed seed list, which is exactly what we need until real parsers land.

pub mod parser;
pub mod saga;

use anyhow::{Result, ensure};
use evento::{Executor, ReadAggregator, cursor::Args};
use serde::{Deserialize, Serialize};

pub use parser::{ParsedCandidate, RecipeParser, SeedParser};

/// Which kind of source the user picked on the upload screen.
///
/// The slug is kept stable because it's also what the saga ends up writing
/// into the recipe's provenance string (`import:url`, `import:file`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportSource {
    Url,
    Photo,
    Text,
    Share,
    File,
}

impl ImportSource {
    pub fn slug(self) -> &'static str {
        match self {
            ImportSource::Url => "url",
            ImportSource::Photo => "photo",
            ImportSource::Text => "text",
            ImportSource::Share => "share",
            ImportSource::File => "file",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "url" => Some(ImportSource::Url),
            "photo" => Some(ImportSource::Photo),
            "text" => Some(ImportSource::Text),
            "share" => Some(ImportSource::Share),
            "file" => Some(ImportSource::File),
            _ => None,
        }
    }
}

/// UI-level stage label. Derived from the event log, NOT stored as a column
/// on the aggregate — the events alone are the source of truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportStage {
    Started,
    Previewed,
    Completed,
    Failed,
}

impl ImportStage {
    pub fn slug(self) -> &'static str {
        match self {
            ImportStage::Started => "started",
            ImportStage::Previewed => "previewed",
            ImportStage::Completed => "completed",
            ImportStage::Failed => "failed",
        }
    }
}

/// All events for the `RecipeImport` aggregate.
#[evento::aggregator]
pub enum RecipeImport {
    /// The user picked a source on the upload screen.
    ImportStarted {
        owner_id: String,
        /// Source slug (see [`ImportSource::slug`]).
        source: String,
        /// Display label for the source (e.g. "grandmas-recipes.json · 12.4 KB").
        source_label: String,
    },
    /// The parser produced a candidate list. Triggered by the saga reacting
    /// to `ImportStarted`.
    ImportPreviewed { candidates: Vec<ParsedCandidate> },
    /// The user picked a subset of the candidates and confirmed the import.
    ImportConfirmed { picked_ids: Vec<String> },
    /// The saga finished drafting all picked recipes. `recipe_ids` is the list
    /// of *new* `Recipe` aggregate IDs created from this import (parallel to
    /// `picked_ids`, in the same order).
    ImportCompleted { recipe_ids: Vec<String> },
    /// Terminal failure. `reason` is human-readable; the UI surfaces it as-is.
    ImportFailed { reason: String },
}

// ── Commands ──────────────────────────────────────────────────────────────

/// User picked a source — start a new import. Returns the new aggregate id.
#[derive(Debug, Clone)]
pub struct StartImport {
    pub owner_id: String,
    pub source: ImportSource,
    pub source_label: String,
}

#[tracing::instrument(name = "import.start", skip(executor), fields(owner = %cmd.owner_id, source = cmd.source.slug()))]
pub async fn start_import<E: Executor>(cmd: StartImport, executor: &E) -> Result<String> {
    ensure!(!cmd.owner_id.trim().is_empty(), "owner is required");

    let routing_key = format!("user:{}", cmd.owner_id);
    let event = ImportStarted {
        owner_id: cmd.owner_id.clone(),
        source: cmd.source.slug().to_owned(),
        source_label: cmd.source_label,
    };

    let id = evento::create()
        .event(&event)
        .routing_key(routing_key)
        .requested_by(&cmd.owner_id)
        .commit(executor)
        .await
        .map_err(|e| anyhow::anyhow!("start_import commit failed: {e}"))?;

    tracing::info!(import_id = %id, "import started");
    Ok(id)
}

/// Saga-emitted: record the parser's output.
#[derive(Debug, Clone)]
pub(crate) struct RecordPreview {
    pub import_id: String,
    pub candidates: Vec<ParsedCandidate>,
}

#[tracing::instrument(name = "import.preview", skip(executor, cmd), fields(import_id = %cmd.import_id, candidates = cmd.candidates.len()))]
pub(crate) async fn record_preview<E: Executor>(cmd: RecordPreview, executor: &E) -> Result<()> {
    let original_version = current_version::<E>(&cmd.import_id, executor).await?;
    let event = ImportPreviewed {
        candidates: cmd.candidates,
    };

    evento::aggregator(&cmd.import_id)
        .original_version(original_version)
        .event(&event)
        .commit(executor)
        .await
        .map_err(|e| anyhow::anyhow!("record_preview commit failed: {e}"))?;
    Ok(())
}

/// User clicked "Import selected".
#[derive(Debug, Clone)]
pub struct ConfirmImport {
    pub import_id: String,
    pub picked_ids: Vec<String>,
}

#[tracing::instrument(name = "import.confirm", skip(executor, cmd), fields(import_id = %cmd.import_id, picked = cmd.picked_ids.len()))]
pub async fn confirm_import<E: Executor>(cmd: ConfirmImport, executor: &E) -> Result<()> {
    ensure!(!cmd.picked_ids.is_empty(), "pick at least one recipe");

    let original_version = current_version::<E>(&cmd.import_id, executor).await?;
    let event = ImportConfirmed {
        picked_ids: cmd.picked_ids,
    };

    evento::aggregator(&cmd.import_id)
        .original_version(original_version)
        .event(&event)
        .commit(executor)
        .await
        .map_err(|e| anyhow::anyhow!("confirm_import commit failed: {e}"))?;
    Ok(())
}

/// Saga-emitted: record the completion of an import.
#[derive(Debug, Clone)]
pub(crate) struct RecordCompletion {
    pub import_id: String,
    pub recipe_ids: Vec<String>,
}

#[tracing::instrument(name = "import.complete", skip(executor, cmd), fields(import_id = %cmd.import_id, drafted = cmd.recipe_ids.len()))]
pub(crate) async fn record_completion<E: Executor>(
    cmd: RecordCompletion,
    executor: &E,
) -> Result<()> {
    let original_version = current_version::<E>(&cmd.import_id, executor).await?;
    let event = ImportCompleted {
        recipe_ids: cmd.recipe_ids,
    };

    evento::aggregator(&cmd.import_id)
        .original_version(original_version)
        .event(&event)
        .commit(executor)
        .await
        .map_err(|e| anyhow::anyhow!("record_completion commit failed: {e}"))?;
    Ok(())
}

/// Saga-emitted: record a terminal failure.
#[derive(Debug, Clone)]
pub(crate) struct RecordFailure {
    pub import_id: String,
    pub reason: String,
}

#[tracing::instrument(name = "import.fail", skip(executor, cmd), fields(import_id = %cmd.import_id))]
pub(crate) async fn record_failure<E: Executor>(cmd: RecordFailure, executor: &E) -> Result<()> {
    let original_version = current_version::<E>(&cmd.import_id, executor).await?;
    let event = ImportFailed { reason: cmd.reason };

    evento::aggregator(&cmd.import_id)
        .original_version(original_version)
        .event(&event)
        .commit(executor)
        .await
        .map_err(|e| anyhow::anyhow!("record_failure commit failed: {e}"))?;
    Ok(())
}

/// Tiny helper: read the latest version for an existing aggregate so we can
/// pass `original_version` to a follow-up `evento::aggregator(...)` call. The
/// aggregate's current version is the version of its last event.
///
/// We bypass `Projection` here because all we need is the count — folding the
/// state is wasted work for the saga's tiny payloads and we'd just throw the
/// result away.
async fn current_version<E: Executor>(import_id: &str, executor: &E) -> Result<u16> {
    let read = executor
        .read(
            Some(vec![ReadAggregator::id(
                <ImportStarted as evento::Aggregator>::aggregator_type(),
                import_id,
            )]),
            None,
            Args::backward(1, None),
        )
        .await?;

    Ok(read
        .edges
        .into_iter()
        .next()
        .map(|edge| edge.node.version)
        .unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_source_round_trips() {
        for slug in ["url", "photo", "text", "share", "file"] {
            let parsed = ImportSource::parse(slug).expect("known slug");
            assert_eq!(parsed.slug(), slug);
        }
        assert!(ImportSource::parse("ftp").is_none());
    }
}
