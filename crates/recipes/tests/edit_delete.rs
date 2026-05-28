//! End-to-end tests for the per-section edit + delete commands.
//!
//! Builds a real in-memory SQLite executor, starts the recipes_view
//! subscription, drafts a recipe, then exercises every edit command and
//! verifies the projection row converges to the expected state.

use std::{sync::Arc, time::Duration};

use imkitchen_recipes::{
    import::{RecipeParser, SeedParser},
    migrations,
    projection::recipes_view::{self, RecipesQuery},
    recipe::{
        DeleteRecipe, DraftRecipe, IngredientFact, MealType, Provenance, RecategorizeRecipe,
        RedescribeRecipe, RenameRecipe, ReplaceIngredients, ReplaceSteps, RetagRecipe,
        RetimeRecipe, StepFact, Unit, delete_recipe, draft_recipe, recategorize_recipe,
        redescribe_recipe, rename_recipe, replace_ingredients, replace_steps, retag_recipe,
        retime_recipe,
    },
    subscriptions::start_all,
};
use sqlx::{Sqlite, SqlitePool, sqlite::SqlitePoolOptions};
use sqlx_migrator::{Info, Migrate, Migrator, Plan};

async fn setup() -> (evento::Sqlite, SqlitePool) {
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

fn good_draft(owner_id: &str) -> DraftRecipe {
    DraftRecipe {
        owner_id: owner_id.to_owned(),
        title: "Lentil Stew".into(),
        meal_type: MealType::Main,
        cuisine: "Lebanese".into(),
        emoji: "🍲".into(),
        prep_minutes: 10,
        cook_minutes: 25,
        servings: 4,
        difficulty: "Easy".into(),
        description: "Warming weeknight stew.".into(),
        tags: vec!["Vegan".into()],
        ingredients: vec![IngredientFact {
            name: "Red lentils".into(),
            quantity: Some(1.0),
            unit: Unit::Cup,
        }],
        steps: vec![StepFact {
            wait_minutes: 5,
            text: "Soften onion and garlic in olive oil.".into(),
        }],
        provenance: Provenance::manual(),
    }
}

/// Poll the projection until `predicate(row)` is true or we give up. Returns
/// `Some(row)` when the predicate is satisfied (or the row vanished and we
/// want that), otherwise `None`.
async fn wait_for<F>(
    pool: &SqlitePool,
    owner_id: &str,
    recipe_id: &str,
    mut predicate: F,
) -> Option<recipes_view::RecipeRow>
where
    F: FnMut(&Option<recipes_view::RecipeRow>) -> bool,
{
    for _ in 0..60 {
        let row = recipes_view::find_for_owner(pool, owner_id, recipe_id)
            .await
            .ok()
            .flatten();
        if predicate(&row) {
            return row;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    None
}

#[tokio::test(flavor = "multi_thread")]
async fn edit_commands_propagate_to_projection() {
    let (executor, pool) = setup().await;
    let parser: Arc<dyn RecipeParser> = Arc::new(SeedParser);
    let subs = start_all(&executor, pool.clone(), parser).await.expect("start");

    let owner = "u-edit";
    let recipe_id = draft_recipe(good_draft(owner), &executor).await.expect("draft");

    // Wait for the projection to ingest the draft.
    let row = wait_for(&pool, owner, &recipe_id, |r| r.is_some())
        .await
        .expect("draft row not in projection");
    assert_eq!(row.title, "Lentil Stew");
    assert_eq!(row.meal_type, "main");

    // Rename.
    rename_recipe(
        RenameRecipe {
            recipe_id: recipe_id.clone(),
            owner_id: owner.into(),
            new_title: "  Spicy Lentil Stew  ".into(),
        },
        &executor,
    )
    .await
    .expect("rename");
    let row = wait_for(&pool, owner, &recipe_id, |r| {
        r.as_ref().is_some_and(|x| x.title == "Spicy Lentil Stew")
    })
    .await
    .expect("rename not propagated");
    assert_eq!(row.title, "Spicy Lentil Stew");

    // Recategorize — clear emoji to verify default-from-meal-type.
    recategorize_recipe(
        RecategorizeRecipe {
            recipe_id: recipe_id.clone(),
            owner_id: owner.into(),
            meal_type: MealType::Dessert,
            cuisine: "French".into(),
            emoji: String::new(),
        },
        &executor,
    )
    .await
    .expect("recategorize");
    let row = wait_for(&pool, owner, &recipe_id, |r| {
        r.as_ref().is_some_and(|x| x.meal_type == "dessert")
    })
    .await
    .expect("recategorize not propagated");
    assert_eq!(row.cuisine, "French");
    assert_eq!(row.emoji, MealType::Dessert.default_emoji());

    // Retime — verifies `time_minutes` recompute (prep + cook = 5 + 40 = 45).
    retime_recipe(
        RetimeRecipe {
            recipe_id: recipe_id.clone(),
            owner_id: owner.into(),
            prep_minutes: 5,
            cook_minutes: 40,
            servings: 6,
            difficulty: "Hard".into(),
        },
        &executor,
    )
    .await
    .expect("retime");
    let row = wait_for(&pool, owner, &recipe_id, |r| {
        r.as_ref().is_some_and(|x| x.time_minutes == 45)
    })
    .await
    .expect("retime not propagated");
    assert_eq!(row.servings, 6);
    assert_eq!(row.difficulty, "Hard");

    // Redescribe.
    redescribe_recipe(
        RedescribeRecipe {
            recipe_id: recipe_id.clone(),
            owner_id: owner.into(),
            description: "Reworked weeknight version.".into(),
        },
        &executor,
    )
    .await
    .expect("redescribe");
    let row = wait_for(&pool, owner, &recipe_id, |r| {
        r.as_ref()
            .is_some_and(|x| x.description == "Reworked weeknight version.")
    })
    .await
    .expect("redescribe not propagated");
    assert_eq!(row.description, "Reworked weeknight version.");

    // Retag.
    retag_recipe(
        RetagRecipe {
            recipe_id: recipe_id.clone(),
            owner_id: owner.into(),
            tags: vec!["Vegetarian".into(), "One-pot".into()],
        },
        &executor,
    )
    .await
    .expect("retag");
    let row = wait_for(&pool, owner, &recipe_id, |r| {
        r.as_ref().is_some_and(|x| x.tags().len() == 2)
    })
    .await
    .expect("retag not propagated");
    let tags = row.tags();
    assert!(tags.contains(&"Vegetarian".to_string()));
    assert!(tags.contains(&"One-pot".to_string()));

    // Replace ingredients — include a blank row to verify it's dropped. We
    // discriminate by name (not count) because the seed row already has 1
    // ingredient — checking count alone would race.
    replace_ingredients(
        ReplaceIngredients {
            recipe_id: recipe_id.clone(),
            owner_id: owner.into(),
            ingredients: vec![
                IngredientFact::default(),
                IngredientFact {
                    name: "Coconut milk".into(),
                    quantity: Some(400.0),
                    unit: Unit::Ml,
                },
                IngredientFact::default(),
            ],
        },
        &executor,
    )
    .await
    .expect("replace_ingredients");
    let row = wait_for(&pool, owner, &recipe_id, |r| {
        r.as_ref().is_some_and(|x| {
            let ings = x.ingredients();
            ings.len() == 1 && ings[0].name == "Coconut milk"
        })
    })
    .await
    .expect("ingredients not propagated");
    let ings = row.ingredients();
    assert_eq!(ings[0].name, "Coconut milk");
    assert_eq!(ings[0].unit, Unit::Ml);

    // Replace steps.
    replace_steps(
        ReplaceSteps {
            recipe_id: recipe_id.clone(),
            owner_id: owner.into(),
            steps: vec![
                StepFact {
                    wait_minutes: 0,
                    text: "Sauté aromatics.".into(),
                },
                StepFact {
                    wait_minutes: 30,
                    text: "Simmer covered.".into(),
                },
            ],
        },
        &executor,
    )
    .await
    .expect("replace_steps");
    let row = wait_for(&pool, owner, &recipe_id, |r| {
        r.as_ref().is_some_and(|x| x.steps().len() == 2)
    })
    .await
    .expect("steps not propagated");
    assert_eq!(row.steps()[1].text, "Simmer covered.");

    // Delete — projection row should disappear once the `RecipeDeleted`
    // event propagates through the subscription.
    delete_recipe(
        DeleteRecipe {
            recipe_id: recipe_id.clone(),
            owner_id: owner.into(),
        },
        &executor,
    )
    .await
    .expect("delete");
    let gone = wait_for(&pool, owner, &recipe_id, |r| r.is_none()).await;
    assert!(gone.is_none(), "row should be absent after delete");

    // Any further command must fail with `recipe deleted`.
    let err = rename_recipe(
        RenameRecipe {
            recipe_id: recipe_id.clone(),
            owner_id: owner.into(),
            new_title: "Should not work".into(),
        },
        &executor,
    )
    .await
    .expect_err("post-delete rename should fail");
    assert!(err.to_string().contains("recipe deleted"), "{err}");

    subs.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn owner_check_blocks_other_users() {
    let (executor, pool) = setup().await;
    let parser: Arc<dyn RecipeParser> = Arc::new(SeedParser);
    let subs = start_all(&executor, pool.clone(), parser).await.expect("start");

    let owner = "u-alice";
    let recipe_id = draft_recipe(good_draft(owner), &executor).await.expect("draft");
    wait_for(&pool, owner, &recipe_id, |r| r.is_some())
        .await
        .expect("draft row");

    // Mallory tries to rename Alice's recipe.
    let err = rename_recipe(
        RenameRecipe {
            recipe_id: recipe_id.clone(),
            owner_id: "u-mallory".into(),
            new_title: "Pwned".into(),
        },
        &executor,
    )
    .await
    .expect_err("non-owner rename must fail");
    assert!(err.to_string().contains("not owner"), "{err}");

    // Delete also rejected.
    let err = delete_recipe(
        DeleteRecipe {
            recipe_id: recipe_id.clone(),
            owner_id: "u-mallory".into(),
        },
        &executor,
    )
    .await
    .expect_err("non-owner delete must fail");
    assert!(err.to_string().contains("not owner"), "{err}");

    // Alice's row is still intact and unmodified.
    let row = recipes_view::find_for_owner(&pool, owner, &recipe_id)
        .await
        .expect("read")
        .expect("row present");
    assert_eq!(row.title, "Lentil Stew");

    subs.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_is_idempotent() {
    let (executor, pool) = setup().await;
    let parser: Arc<dyn RecipeParser> = Arc::new(SeedParser);
    let subs = start_all(&executor, pool.clone(), parser).await.expect("start");

    let owner = "u-idem";
    let recipe_id = draft_recipe(good_draft(owner), &executor).await.expect("draft");
    wait_for(&pool, owner, &recipe_id, |r| r.is_some())
        .await
        .expect("draft row");

    delete_recipe(
        DeleteRecipe {
            recipe_id: recipe_id.clone(),
            owner_id: owner.into(),
        },
        &executor,
    )
    .await
    .expect("first delete");

    // Wait for the projection to drop the row.
    let gone = wait_for(&pool, owner, &recipe_id, |r| r.is_none()).await;
    assert!(gone.is_none(), "row should be absent after first delete");

    // Second delete returns Ok — no error, no extra event written. The
    // exact event count is an implementation detail; we just want the
    // second call to succeed.
    delete_recipe(
        DeleteRecipe {
            recipe_id: recipe_id.clone(),
            owner_id: owner.into(),
        },
        &executor,
    )
    .await
    .expect("second delete is a no-op");

    // Row still gone.
    let row = recipes_view::find_for_owner(&pool, owner, &recipe_id)
        .await
        .expect("read");
    assert!(row.is_none());

    subs.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn list_excludes_owners_who_dont_own_recipes() {
    // Sanity check that owner scoping in `list_for_owner` matches the new
    // owner-id baked into `RecipeDrafted`.
    let (executor, pool) = setup().await;
    let parser: Arc<dyn RecipeParser> = Arc::new(SeedParser);
    let subs = start_all(&executor, pool.clone(), parser).await.expect("start");

    let owner_a = "u-a";
    let owner_b = "u-b";
    let _ra = draft_recipe(good_draft(owner_a), &executor).await.expect("draft a");
    let _rb = draft_recipe(
        DraftRecipe {
            title: "B's pancakes".into(),
            ..good_draft(owner_b)
        },
        &executor,
    )
    .await
    .expect("draft b");

    // Wait for both rows.
    wait_for(&pool, owner_a, &_ra, |r| r.is_some()).await.expect("a row");
    wait_for(&pool, owner_b, &_rb, |r| r.is_some()).await.expect("b row");

    let a_rows = recipes_view::list_for_owner(&pool, owner_a, &RecipesQuery::default())
        .await
        .expect("list a");
    let b_rows = recipes_view::list_for_owner(&pool, owner_b, &RecipesQuery::default())
        .await
        .expect("list b");
    assert_eq!(a_rows.len(), 1);
    assert_eq!(b_rows.len(), 1);
    assert_ne!(a_rows[0].title, b_rows[0].title);

    subs.shutdown().await.expect("shutdown");
}
