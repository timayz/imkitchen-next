//! Subscription wiring — keeps the read-side tables in sync with the event
//! stream and starts the import saga.
//!
//! Three subscriptions get spawned at boot:
//!
//! 1. `recipes.projection.recipes_view`  — every `RecipeDrafted` → upsert
//!    `recipes_view` row.
//! 2. `recipes.projection.recipe_imports_view` — every `RecipeImport` event
//!    → update `recipe_imports_view`.
//! 3. The import saga (see `import::saga`).
//!
//! All three set `.all()` so they ignore per-user routing keys.

use std::sync::Arc;

use anyhow::Result;
use evento::{
    Executor,
    metadata::Event,
    subscription::{Context, Subscription, SubscriptionBuilder},
};
use sqlx::SqlitePool;

use crate::{
    import::{
        ImportCompleted, ImportConfirmed, ImportFailed, ImportPreviewed, ImportStarted,
        saga::start as start_saga,
    },
    projection::{recipe_imports_view, recipes_view},
    recipe::{
        IngredientsReplaced, RecipeDeleted, RecipeDrafted, RecipeRecategorized, RecipeRedescribed,
        RecipeRenamed, RecipeRetagged, RecipeRetimed, StepsReplaced,
    },
};

/// Started subscriptions, returned so the host can shut them down on signal.
pub struct RecipeSubscriptions {
    pub recipes_view: Subscription,
    pub imports_view: Subscription,
    pub saga: Subscription,
}

impl RecipeSubscriptions {
    pub async fn shutdown(self) -> Result<()> {
        let RecipeSubscriptions {
            recipes_view,
            imports_view,
            saga,
        } = self;
        // Run shutdowns concurrently — none of them depend on each other.
        let (a, b, c) =
            tokio::join!(recipes_view.shutdown(), imports_view.shutdown(), saga.shutdown());
        a?;
        b?;
        c?;
        Ok(())
    }
}

#[tracing::instrument(name = "projection.recipe_drafted", skip(ctx, event), fields(recipe_id = %event.aggregator_id))]
#[evento::subscription]
async fn on_recipe_drafted<E: Executor>(
    ctx: &Context<'_, E>,
    event: Event<RecipeDrafted>,
) -> Result<()> {
    let pool: WritePool = ctx.extract();
    recipes_view::apply_drafted(&pool.0, &event).await
}

#[tracing::instrument(name = "projection.recipe_renamed", skip(ctx, event), fields(recipe_id = %event.aggregator_id))]
#[evento::subscription]
async fn on_recipe_renamed<E: Executor>(
    ctx: &Context<'_, E>,
    event: Event<RecipeRenamed>,
) -> Result<()> {
    let pool: WritePool = ctx.extract();
    recipes_view::apply_renamed(&pool.0, &event).await
}

#[tracing::instrument(name = "projection.recipe_recategorized", skip(ctx, event), fields(recipe_id = %event.aggregator_id))]
#[evento::subscription]
async fn on_recipe_recategorized<E: Executor>(
    ctx: &Context<'_, E>,
    event: Event<RecipeRecategorized>,
) -> Result<()> {
    let pool: WritePool = ctx.extract();
    recipes_view::apply_recategorized(&pool.0, &event).await
}

#[tracing::instrument(name = "projection.recipe_retimed", skip(ctx, event), fields(recipe_id = %event.aggregator_id))]
#[evento::subscription]
async fn on_recipe_retimed<E: Executor>(
    ctx: &Context<'_, E>,
    event: Event<RecipeRetimed>,
) -> Result<()> {
    let pool: WritePool = ctx.extract();
    recipes_view::apply_retimed(&pool.0, &event).await
}

#[tracing::instrument(name = "projection.recipe_redescribed", skip(ctx, event), fields(recipe_id = %event.aggregator_id))]
#[evento::subscription]
async fn on_recipe_redescribed<E: Executor>(
    ctx: &Context<'_, E>,
    event: Event<RecipeRedescribed>,
) -> Result<()> {
    let pool: WritePool = ctx.extract();
    recipes_view::apply_redescribed(&pool.0, &event).await
}

#[tracing::instrument(name = "projection.recipe_retagged", skip(ctx, event), fields(recipe_id = %event.aggregator_id))]
#[evento::subscription]
async fn on_recipe_retagged<E: Executor>(
    ctx: &Context<'_, E>,
    event: Event<RecipeRetagged>,
) -> Result<()> {
    let pool: WritePool = ctx.extract();
    recipes_view::apply_retagged(&pool.0, &event).await
}

#[tracing::instrument(name = "projection.ingredients_replaced", skip(ctx, event), fields(recipe_id = %event.aggregator_id))]
#[evento::subscription]
async fn on_ingredients_replaced<E: Executor>(
    ctx: &Context<'_, E>,
    event: Event<IngredientsReplaced>,
) -> Result<()> {
    let pool: WritePool = ctx.extract();
    recipes_view::apply_ingredients_replaced(&pool.0, &event).await
}

#[tracing::instrument(name = "projection.steps_replaced", skip(ctx, event), fields(recipe_id = %event.aggregator_id))]
#[evento::subscription]
async fn on_steps_replaced<E: Executor>(
    ctx: &Context<'_, E>,
    event: Event<StepsReplaced>,
) -> Result<()> {
    let pool: WritePool = ctx.extract();
    recipes_view::apply_steps_replaced(&pool.0, &event).await
}

#[tracing::instrument(name = "projection.recipe_deleted", skip(ctx, event), fields(recipe_id = %event.aggregator_id))]
#[evento::subscription]
async fn on_recipe_deleted<E: Executor>(
    ctx: &Context<'_, E>,
    event: Event<RecipeDeleted>,
) -> Result<()> {
    let pool: WritePool = ctx.extract();
    recipes_view::apply_deleted(&pool.0, &event).await
}

#[tracing::instrument(name = "projection.import_started", skip(ctx, event), fields(import_id = %event.aggregator_id))]
#[evento::subscription]
async fn on_import_started<E: Executor>(
    ctx: &Context<'_, E>,
    event: Event<ImportStarted>,
) -> Result<()> {
    let pool: WritePool = ctx.extract();
    recipe_imports_view::apply_started(&pool.0, &event).await
}

#[tracing::instrument(name = "projection.import_previewed", skip(ctx, event), fields(import_id = %event.aggregator_id))]
#[evento::subscription]
async fn on_import_previewed<E: Executor>(
    ctx: &Context<'_, E>,
    event: Event<ImportPreviewed>,
) -> Result<()> {
    let pool: WritePool = ctx.extract();
    recipe_imports_view::apply_previewed(&pool.0, &event).await
}

#[tracing::instrument(name = "projection.import_confirmed", skip(ctx, event), fields(import_id = %event.aggregator_id))]
#[evento::subscription]
async fn on_import_confirmed<E: Executor>(
    ctx: &Context<'_, E>,
    event: Event<ImportConfirmed>,
) -> Result<()> {
    let pool: WritePool = ctx.extract();
    recipe_imports_view::apply_confirmed(&pool.0, &event).await
}

#[tracing::instrument(name = "projection.import_completed", skip(ctx, event), fields(import_id = %event.aggregator_id))]
#[evento::subscription]
async fn on_import_completed<E: Executor>(
    ctx: &Context<'_, E>,
    event: Event<ImportCompleted>,
) -> Result<()> {
    let pool: WritePool = ctx.extract();
    recipe_imports_view::apply_completed(&pool.0, &event).await
}

#[tracing::instrument(name = "projection.import_failed", skip(ctx, event), fields(import_id = %event.aggregator_id))]
#[evento::subscription]
async fn on_import_failed<E: Executor>(
    ctx: &Context<'_, E>,
    event: Event<ImportFailed>,
) -> Result<()> {
    let pool: WritePool = ctx.extract();
    recipe_imports_view::apply_failed(&pool.0, &event).await
}

/// Cheap-to-clone wrapper around the projection-write `SqlitePool` so the
/// subscription context can carry it via `TypeId`. (Reads at the web layer
/// happen against the separate read-only pool.)
#[derive(Clone)]
struct WritePool(SqlitePool);

/// Spawn all three subscriptions. Caller owns the returned handles and
/// should call [`RecipeSubscriptions::shutdown`] on graceful shutdown.
///
/// `write_pool` is the SQLite pool the projections write into (same database
/// as the event store — see `crates/db::create_write_pool`). The web
/// handlers query via the read-only `read_pool`; both pools point at the
/// same database file.
pub async fn start_all<E: Executor + Clone>(
    executor: &E,
    write_pool: SqlitePool,
    parser: Arc<dyn crate::import::RecipeParser>,
) -> Result<RecipeSubscriptions> {
    let pool = WritePool(write_pool);

    let recipes_view = SubscriptionBuilder::<E>::new("recipes.projection.recipes_view")
        .all()
        .data(pool.clone())
        .handler(on_recipe_drafted())
        .handler(on_recipe_renamed())
        .handler(on_recipe_recategorized())
        .handler(on_recipe_retimed())
        .handler(on_recipe_redescribed())
        .handler(on_recipe_retagged())
        .handler(on_ingredients_replaced())
        .handler(on_steps_replaced())
        .handler(on_recipe_deleted())
        .accept_failure()
        .start(executor)
        .await?;

    let imports_view = SubscriptionBuilder::<E>::new("recipes.projection.recipe_imports_view")
        .all()
        .data(pool)
        .handler(on_import_started())
        .handler(on_import_previewed())
        .handler(on_import_confirmed())
        .handler(on_import_completed())
        .handler(on_import_failed())
        .accept_failure()
        .start(executor)
        .await?;

    let saga = start_saga(parser, executor).await?;

    Ok(RecipeSubscriptions {
        recipes_view,
        imports_view,
        saga,
    })
}
