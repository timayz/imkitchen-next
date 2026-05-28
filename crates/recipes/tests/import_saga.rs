//! End-to-end test for the import saga.
//!
//! Builds a real in-memory SQLite executor, runs the saga subscription
//! against it, fires `StartImport` + `ConfirmImport`, and verifies the
//! eventual state of the read models.

use std::{sync::Arc, time::Duration};

use imkitchen_recipes::{
    import::{
        ConfirmImport, ImportSource, RecipeParser, SeedParser, StartImport, confirm_import,
        parser::{DraftMaterial, ParsedCandidate},
        start_import,
    },
    migrations,
    projection::{recipe_imports_view, recipes_view},
    recipe::MealType,
    subscriptions::start_all,
};
use sqlx::{Sqlite, SqlitePool, sqlite::SqlitePoolOptions};
use sqlx_migrator::{Info, Migrate, Migrator, Plan};

async fn setup() -> (evento::Sqlite, SqlitePool) {
    // Single shared in-memory DB so the read pool sees what the event store
    // writes via the same connection space.
    let pool: SqlitePool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("sqlite pool");

    let mut conn = pool.acquire().await.expect("acquire");
    evento::sql_migrator::new::<Sqlite>()
        .expect("evento migrator")
        .run(&mut *conn, &Plan::apply_all())
        .await
        .expect("evento migrations");

    let mut migrator: Migrator<Sqlite> = Migrator::default();
    migrator
        .add_migrations(migrations::migrations())
        .expect("recipes migrations");
    migrator
        .run(&mut *conn, &Plan::apply_all())
        .await
        .expect("recipes migrations apply");
    drop(conn);

    let executor: evento::Sqlite = pool.clone().into();
    (executor, pool)
}

/// Picks-only deterministic stub that materializes one ingredient + step per
/// candidate. We don't reuse `SeedParser` here so the test stays independent
/// of changes to the seed data.
struct TestParser;

#[async_trait::async_trait]
impl RecipeParser for TestParser {
    async fn parse(&self, _source: ImportSource, _label: &str) -> anyhow::Result<Vec<ParsedCandidate>> {
        Ok(vec![
            ParsedCandidate {
                id: "p1".into(),
                title: "Test Recipe One".into(),
                emoji: "🥘".into(),
                meal_type: "main".into(),
                ingredient_count: 1,
                step_count: 1,
                warn: None,
                broken: false,
                selected: true,
            },
            ParsedCandidate {
                id: "p2".into(),
                title: "Broken Row".into(),
                emoji: "❌".into(),
                meal_type: "main".into(),
                ingredient_count: 0,
                step_count: 0,
                warn: Some("missing data".into()),
                broken: true,
                selected: false,
            },
        ])
    }

    async fn parse_file(
        &self,
        _file_name: &str,
        _content_type: &str,
        _bytes: &[u8],
    ) -> anyhow::Result<Vec<ParsedCandidate>> {
        self.parse(ImportSource::File, "").await
    }

    async fn materialize(&self, c: &ParsedCandidate) -> anyhow::Result<DraftMaterial> {
        Ok(DraftMaterial {
            title: c.title.clone(),
            meal_type: MealType::Main,
            cuisine: "Test".into(),
            emoji: c.emoji.clone(),
            prep_minutes: 5,
            cook_minutes: 15,
            servings: 2,
            difficulty: "Easy".into(),
            description: String::new(),
            tags: vec![],
            ingredients: vec![imkitchen_recipes::IngredientFact {
                name: "Salt".into(),
                quantity: None,
                unit: imkitchen_recipes::Unit::Pinch,
            }],
            steps: vec![imkitchen_recipes::StepFact {
                wait_minutes: 5,
                text: "Combine and serve.".into(),
            }],
        })
    }
}

async fn wait_for_stage(pool: &SqlitePool, import_id: &str, target: &str) -> bool {
    for _ in 0..60 {
        if let Ok(Some(row)) = recipe_imports_view::find(pool, import_id).await
            && row.stage == target
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

#[tokio::test(flavor = "multi_thread")]
async fn import_saga_drafts_picked_recipes() {
    let (executor, pool) = setup().await;
    let parser: Arc<dyn RecipeParser> = Arc::new(TestParser);

    let subs = start_all(&executor, pool.clone(), parser)
        .await
        .expect("start subscriptions");

    let import_id = start_import(
        StartImport {
            owner_id: "user-1".into(),
            source: ImportSource::File,
            source_label: "test.json".into(),
        },
        &executor,
    )
    .await
    .expect("start_import");

    assert!(
        wait_for_stage(&pool, &import_id, "previewed").await,
        "import never reached previewed stage"
    );

    // Confirm only the valid candidate; the broken one should be skipped by
    // the saga.
    confirm_import(
        ConfirmImport {
            import_id: import_id.clone(),
            picked_ids: vec!["p1".into(), "p2".into()],
        },
        &executor,
    )
    .await
    .expect("confirm_import");

    assert!(
        wait_for_stage(&pool, &import_id, "completed").await,
        "import never reached completed stage"
    );

    let row = recipe_imports_view::find(&pool, &import_id)
        .await
        .expect("find import")
        .expect("import row exists");
    assert_eq!(row.stage, "completed");

    let recipe_ids = row.recipe_ids();
    assert_eq!(recipe_ids.len(), 1, "broken candidate should be skipped");

    // The drafted recipe should be in `recipes_view` for the owner.
    let recipes = recipes_view::list_for_owner(
        &pool,
        "user-1",
        &recipes_view::RecipesQuery::default(),
    )
    .await
    .expect("list recipes");
    assert_eq!(recipes.len(), 1);
    assert_eq!(recipes[0].title, "Test Recipe One");

    subs.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn seed_parser_full_flow() {
    let (executor, pool) = setup().await;
    let parser: Arc<dyn RecipeParser> = Arc::new(SeedParser);

    let subs = start_all(&executor, pool.clone(), parser)
        .await
        .expect("start subscriptions");

    let import_id = start_import(
        StartImport {
            owner_id: "user-seed".into(),
            source: ImportSource::File,
            source_label: "grandmas.json".into(),
        },
        &executor,
    )
    .await
    .expect("start_import");

    assert!(
        wait_for_stage(&pool, &import_id, "previewed").await,
        "seed import never previewed"
    );

    // The seed has 8 candidates; pick all of them. The saga should draft the
    // 7 valid ones and skip the broken i8.
    let row = recipe_imports_view::find(&pool, &import_id)
        .await
        .expect("find")
        .expect("import row");
    let all_ids: Vec<String> = row.candidates().into_iter().map(|c| c.id).collect();
    assert_eq!(all_ids.len(), 8);

    confirm_import(
        ConfirmImport {
            import_id: import_id.clone(),
            picked_ids: all_ids,
        },
        &executor,
    )
    .await
    .expect("confirm");

    assert!(
        wait_for_stage(&pool, &import_id, "completed").await,
        "seed import never completed"
    );

    let recipes = recipes_view::list_for_owner(
        &pool,
        "user-seed",
        &recipes_view::RecipesQuery::default(),
    )
    .await
    .expect("list");
    assert_eq!(
        recipes.len(),
        7,
        "should draft 7 valid candidates (broken i8 skipped)"
    );

    subs.shutdown().await.expect("shutdown");
}
