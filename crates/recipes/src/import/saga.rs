//! Import process manager (saga).
//!
//! Two reactive steps:
//!
//! 1. `ImportStarted` → call the configured `RecipeParser`, then dispatch
//!    `RecordPreview` (or `RecordFailure` on parser error).
//! 2. `ImportConfirmed` → for each picked candidate, materialize via the
//!    parser and dispatch `DraftRecipe` on the `Recipe` aggregate. When all
//!    picks have been drafted, dispatch `RecordCompletion`. If any pick
//!    fails mid-way, dispatch `RecordFailure` — already-drafted recipes
//!    remain in place (see `super::mod` for the rationale).
//!
//! The saga's only state is the events themselves. We re-read the import
//! aggregate via a small projection when we need to recover the candidates
//! list (the `ImportConfirmed` handler only carries the picked IDs, not the
//! full candidates).

use std::sync::Arc;

use anyhow::{Result, anyhow};
use evento::{
    Executor,
    metadata::Event,
    projection::Projection,
    subscription::{Context, SubscriptionBuilder},
};

use crate::recipe::{DraftRecipe, Provenance, draft_recipe};

use super::{
    ImportConfirmed, ImportStarted, ParsedCandidate, RecipeImport, RecipeParser, RecordCompletion,
    RecordFailure, RecordPreview, record_completion, record_failure, record_preview,
};

/// Shared parser injected into the saga subscription via `.data(...)`.
pub type ParserData = Arc<dyn RecipeParser>;

#[tracing::instrument(name = "saga.on_started", skip(ctx, event), fields(import_id = %event.aggregator_id))]
#[evento::subscription]
async fn on_started<E: Executor>(
    ctx: &Context<'_, E>,
    event: Event<ImportStarted>,
) -> Result<()> {
    let parser: ParserData = ctx.extract();
    let source = match crate::import::ImportSource::parse(&event.data.source) {
        Some(s) => s,
        None => {
            // Unknown source slug — fail fast rather than guess.
            record_failure(
                RecordFailure {
                    import_id: event.aggregator_id.clone(),
                    reason: format!("unknown source `{}`", event.data.source),
                },
                ctx.executor,
            )
            .await?;
            return Ok(());
        }
    };

    match parser.parse(source, &event.data.source_label).await {
        Ok(candidates) => {
            record_preview(
                RecordPreview {
                    import_id: event.aggregator_id.clone(),
                    candidates,
                },
                ctx.executor,
            )
            .await
        }
        Err(err) => {
            tracing::warn!(error = %err, "parser failed");
            record_failure(
                RecordFailure {
                    import_id: event.aggregator_id.clone(),
                    reason: format!("parse failed: {err}"),
                },
                ctx.executor,
            )
            .await
        }
    }
}

#[tracing::instrument(name = "saga.on_confirmed", skip(ctx, event), fields(import_id = %event.aggregator_id))]
#[evento::subscription]
async fn on_confirmed<E: Executor>(
    ctx: &Context<'_, E>,
    event: Event<ImportConfirmed>,
) -> Result<()> {
    let parser: ParserData = ctx.extract();

    // Reload the import aggregate so we have the candidate list with the full
    // shape (the `ImportConfirmed` event only carries picked IDs, not the
    // candidate bodies). This is the standard CQRS read-your-own-write inside
    // a saga.
    let view = Projection::<E, ImportView>::new::<RecipeImport>(&event.aggregator_id)
        .handler(on_started_view())
        .handler(on_previewed_view())
        .handler(on_confirmed_view())
        .handler(on_completed_view())
        .handler(on_failed_view())
        .execute(ctx.executor)
        .await?
        .ok_or_else(|| anyhow!("import {} not found", event.aggregator_id))?;

    if matches!(view.terminal, Terminal::Failed | Terminal::Completed) {
        // Already terminal — replay can deliver `ImportConfirmed` twice (e.g.
        // a subscription retry after the completion event was already
        // written). Treat as a no-op.
        return Ok(());
    }

    let mut drafted = Vec::with_capacity(event.data.picked_ids.len());
    for picked_id in &event.data.picked_ids {
        let Some(candidate) = view.candidates.iter().find(|c| c.id == *picked_id) else {
            tracing::warn!(picked_id = %picked_id, "picked candidate not in preview list — skipping");
            continue;
        };
        if candidate.broken {
            tracing::warn!(picked_id = %picked_id, "picked candidate is broken — skipping");
            continue;
        }

        match parser.materialize(candidate).await {
            Ok(material) => {
                let cmd = DraftRecipe {
                    owner_id: view.owner_id.clone(),
                    title: material.title,
                    meal_type: material.meal_type,
                    cuisine: material.cuisine,
                    emoji: material.emoji,
                    prep_minutes: material.prep_minutes,
                    cook_minutes: material.cook_minutes,
                    servings: material.servings,
                    difficulty: material.difficulty,
                    description: material.description,
                    tags: material.tags,
                    ingredients: material.ingredients,
                    steps: material.steps,
                    provenance: Provenance::from_import(&view.source, &event.aggregator_id),
                };
                match draft_recipe(cmd, ctx.executor).await {
                    Ok(recipe_id) => drafted.push(recipe_id),
                    Err(err) => {
                        // Compensation policy: do NOT roll back the drafts we
                        // already created — they're real, finished facts the
                        // user explicitly asked for. Surface the failure and
                        // stop processing further picks.
                        return record_failure(
                            RecordFailure {
                                import_id: event.aggregator_id.clone(),
                                reason: format!(
                                    "drafting `{}` failed: {err}",
                                    candidate.title
                                ),
                            },
                            ctx.executor,
                        )
                        .await;
                    }
                }
            }
            Err(err) => {
                return record_failure(
                    RecordFailure {
                        import_id: event.aggregator_id.clone(),
                        reason: format!("materializing `{}` failed: {err}", candidate.title),
                    },
                    ctx.executor,
                )
                .await;
            }
        }
    }

    record_completion(
        RecordCompletion {
            import_id: event.aggregator_id.clone(),
            recipe_ids: drafted,
        },
        ctx.executor,
    )
    .await
}

// ── Inner projection used by `on_confirmed` to recover the candidate list ─

#[evento::projection]
#[derive(bitcode::Encode, bitcode::Decode)]
struct ImportView {
    owner_id: String,
    source: String,
    candidates: Vec<ParsedCandidate>,
    terminal: Terminal,
}

#[derive(Default, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
enum Terminal {
    #[default]
    Pending,
    Completed,
    Failed,
}

#[evento::handler]
async fn on_started_view(
    event: Event<ImportStarted>,
    view: &mut ImportView,
) -> Result<()> {
    view.owner_id = event.data.owner_id.clone();
    view.source = event.data.source.clone();
    Ok(())
}

#[evento::handler]
async fn on_previewed_view(
    event: Event<super::ImportPreviewed>,
    view: &mut ImportView,
) -> Result<()> {
    view.candidates = event.data.candidates.clone();
    Ok(())
}

#[evento::handler]
async fn on_confirmed_view(
    _event: Event<super::ImportConfirmed>,
    _view: &mut ImportView,
) -> Result<()> {
    Ok(())
}

#[evento::handler]
async fn on_completed_view(
    _event: Event<super::ImportCompleted>,
    view: &mut ImportView,
) -> Result<()> {
    view.terminal = Terminal::Completed;
    Ok(())
}

#[evento::handler]
async fn on_failed_view(
    _event: Event<super::ImportFailed>,
    view: &mut ImportView,
) -> Result<()> {
    view.terminal = Terminal::Failed;
    Ok(())
}

// ── Public start helper ───────────────────────────────────────────────────

/// Start the saga subscription. Returns the handle so callers can shut it
/// down gracefully alongside the HTTP server.
pub async fn start<E: Executor + Clone>(
    parser: ParserData,
    executor: &E,
) -> Result<evento::subscription::Subscription> {
    let sub = SubscriptionBuilder::<E>::new("recipes.import.saga")
        // Import events are committed with a per-user routing key so the read
        // models can fan out — the saga itself listens across all users, so
        // we opt into `.all()` to ignore the routing-key filter.
        .all()
        .data::<ParserData>(parser)
        .handler(on_started())
        .handler(on_confirmed())
        .accept_failure()
        .start(executor)
        .await?;
    Ok(sub)
}
