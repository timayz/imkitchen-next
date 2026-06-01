//! Per-section edit + delete handlers for `/recipes/{id}/…`.
//!
//! One GET (`/recipes/{id}/edit`) renders the whole edit page; eight POST
//! handlers each dispatch one narrow command and re-render *just their own
//! section* on success. For TwinSpark partial requests (`Accept:
//! text/html+partial`) the response is the section's `_edit_*.html`
//! fragment, which replaces itself in place. For browser-level posts the
//! response is a 303 to `/recipes/{id}/edit` (the user lands back on the
//! edit page with the new state visible after the projection catches up).
//!
//! Owner scoping: the GET handler 404s for non-owners (it can't tell
//! "doesn't exist" from "not yours" at the projection layer — `find_for_owner`
//! is scoped — so a non-owner naturally sees `not_found.html`). Every POST
//! handler also passes `owner_id` to the command, and the aggregate's
//! `load_and_authorize` check rejects mismatches with `not owner` — so even
//! a hand-crafted POST from a logged-in non-owner can't slip through.

use std::sync::atomic::{AtomicU64, Ordering};

use askama::Template;
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use imkitchen_recipes::{
    projection::recipes_view,
    recipe::{
        DeleteRecipe, IngredientFact as DomainIngredient, MealType, RecategorizeRecipe,
        RedescribeRecipe, RenameRecipe, ReplaceIngredients, ReplaceSteps, RetagRecipe,
        RetimeRecipe, StepFact as DomainStep, Unit, delete_recipe, recategorize_recipe,
        redescribe_recipe, rename_recipe, replace_ingredients, replace_steps, retag_recipe,
        retime_recipe,
    },
};
use serde::Deserialize;

use crate::{
    AppState,
    auth::{Role, User},
    recipes::{NavItem, owner_id},
    recipes_create::{
        DifficultyOption, IngredientInput, IngredientRow, MealTypeOption, StepInput, StepRow,
        TagOption, difficulty_options, meal_type_options, tag_options, unit_options,
    },
};

const TS_PARTIAL_ACCEPT: &str = "text/html+partial";

fn wants_partial(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains(TS_PARTIAL_ACCEPT))
}

// ── Templates ──────────────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "recipes/edit.html")]
pub struct EditRecipePage {
    pub nav_items: Vec<NavItem>,
    pub user_initial: &'static str,
    pub user_name: &'static str,
    pub user_email: &'static str,
    pub user_premium: bool,

    pub recipe_id: String,
    pub title: String,
    pub cuisine: String,
    pub description: String,
    pub prep_minutes: u32,
    pub cook_minutes: u32,
    pub servings: u32,

    pub meal_type_options: Vec<MealTypeOption>,
    pub difficulty_options: Vec<DifficultyOption>,
    pub tag_options: Vec<TagOption>,

    pub ingredient_rows: Vec<IngredientRow>,
    pub step_rows: Vec<StepRow>,

    // Askama compiles included partials into the parent template's
    // namespace, so the page struct has to carry every field that the
    // section partials reference. None of these are populated on the
    // initial GET — they're only set by per-section POST renders.
    pub err: Option<String>,
    pub summary_err: Option<String>,
    pub saved: bool,
    pub confirming: bool,
}

#[derive(Template)]
#[template(path = "recipes/_edit_title.html")]
pub struct EditTitleFragment {
    pub recipe_id: String,
    pub title: String,
    pub err: Option<String>,
    pub saved: bool,
}

#[derive(Template)]
#[template(path = "recipes/_edit_categorization.html")]
pub struct EditCategorizationFragment {
    pub recipe_id: String,
    pub cuisine: String,
    pub meal_type_options: Vec<MealTypeOption>,
    pub saved: bool,
}

#[derive(Template)]
#[template(path = "recipes/_edit_timing.html")]
pub struct EditTimingFragment {
    pub recipe_id: String,
    pub prep_minutes: u32,
    pub cook_minutes: u32,
    pub servings: u32,
    pub difficulty_options: Vec<DifficultyOption>,
    pub err: Option<String>,
    pub saved: bool,
}

#[derive(Template)]
#[template(path = "recipes/_edit_description.html")]
pub struct EditDescriptionFragment {
    pub recipe_id: String,
    pub description: String,
    pub saved: bool,
}

#[derive(Template)]
#[template(path = "recipes/_edit_tags.html")]
pub struct EditTagsFragment {
    pub recipe_id: String,
    pub tag_options: Vec<TagOption>,
    pub saved: bool,
}

#[derive(Template)]
#[template(path = "recipes/_edit_ingredients.html")]
pub struct EditIngredientsFragment {
    pub recipe_id: String,
    pub ingredient_rows: Vec<IngredientRow>,
    pub summary_err: Option<String>,
    pub saved: bool,
}

#[derive(Template)]
#[template(path = "recipes/_edit_steps.html")]
pub struct EditStepsFragment {
    pub recipe_id: String,
    pub step_rows: Vec<StepRow>,
    pub summary_err: Option<String>,
    pub saved: bool,
}

// ── Query / form types ─────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct EditQuery {
    /// `?confirm=1` enables the second-step delete UI (a real form-post with
    /// a destructive button). Without it, the danger-zone shows just a link.
    #[serde(default)]
    pub confirm: u8,
    /// Set to 1 by section POST redirects so the GET renders the "saving…"
    /// loading shell + poll trigger, giving the projection time to catch up.
    #[serde(default)]
    pub awaiting: u8,
    /// Poll iteration counter. Capped at [`ATTEMPT_LIMIT`].
    #[serde(default)]
    pub attempt: u8,
}

/// 300 ms × 5 ≈ 1.5 s of polling — comfortably above the subscription
/// cadence in practice. Past this the partial response carries
/// `ts-location` to drop the `?awaiting=1` and reload cleanly.
pub const ATTEMPT_LIMIT: u8 = 5;

#[derive(Template)]
#[template(path = "recipes/edit_awaiting.html")]
pub struct EditAwaitingPage {
    pub nav_items: Vec<NavItem>,
    pub user_initial: &'static str,
    pub user_name: &'static str,
    pub user_email: &'static str,
    pub user_premium: bool,

    pub recipe_id: String,
    pub attempt: u8,
    pub attempt_limit: u8,
}

#[derive(Template)]
#[template(path = "recipes/_edit_awaiting.html")]
pub struct EditAwaitingFragment {
    pub recipe_id: String,
    pub attempt: u8,
    pub attempt_limit: u8,
}

// ── GET /recipes/{id}/edit ─────────────────────────────────────────────

#[tracing::instrument(name = "recipes.edit.page", skip(state, headers), fields(recipe_id = %id, role = user.role.as_str(), awaiting = q.awaiting, attempt = q.attempt))]
pub async fn edit_page(
    user: User,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<EditQuery>,
) -> Response {
    let owner = owner_id(&user);
    let row = match recipes_view::find_for_owner(&state.read_pool, &owner, &id).await {
        Ok(r) => r,
        Err(err) => {
            tracing::error!(error = %err, "find_for_owner failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response();
        }
    };

    let Some(row) = row else {
        // Non-owners and genuinely-missing recipes look the same here. The
        // brief says: "Owner-only — non-owners get 404 (don't reveal
        // existence)" — `find_for_owner` already does the right thing.
        return not_found_response(&user);
    };

    // Awaiting branch: a section POST just redirected here. Show a loading
    // shell + ts-trigger poll for ATTEMPT_LIMIT iterations, then ts-location
    // back to the clean URL so the browser does a full nav into the freshly-
    // projected edit page.
    if q.awaiting > 0 {
        let attempt = q.attempt;
        if attempt >= ATTEMPT_LIMIT {
            if wants_partial(&headers) {
                let mut resp = (StatusCode::OK, "").into_response();
                let url = format!("/recipes/{}/edit", id);
                if let Ok(value) = axum::http::HeaderValue::from_str(&url) {
                    resp.headers_mut().insert("ts-location", value);
                }
                return resp;
            }
            // Fall through to the full edit page if the browser GETs the
            // shell at the limit (rare — only if the user disabled JS).
        } else {
            if wants_partial(&headers) {
                return render(EditAwaitingFragment {
                    recipe_id: id,
                    attempt,
                    attempt_limit: ATTEMPT_LIMIT,
                });
            }
            let chrome = chrome_for(&user);
            return render(EditAwaitingPage {
                nav_items: chrome.nav_items,
                user_initial: chrome.user_initial,
                user_name: chrome.user_name,
                user_email: chrome.user_email,
                user_premium: chrome.user_premium,
                recipe_id: id,
                attempt,
                attempt_limit: ATTEMPT_LIMIT,
            });
        }
    }

    render(build_edit_page(&user, &row, q.confirm > 0))
}

struct Chrome {
    nav_items: Vec<NavItem>,
    user_initial: &'static str,
    user_name: &'static str,
    user_email: &'static str,
    user_premium: bool,
}

fn chrome_for(user: &User) -> Chrome {
    Chrome {
        nav_items: crate::recipes::nav_items(),
        user_initial: crate::recipes::user_initial(user),
        user_name: "Jenny Rosen",
        user_email: "jenny@imkitchen.app",
        user_premium: matches!(user.role, Role::Premium),
    }
}

fn build_edit_page(user: &User, row: &recipes_view::RecipeRow, confirming: bool) -> EditRecipePage {
    let ingredients = row.ingredients();
    let steps = row.steps();

    let ingredient_rows: Vec<IngredientRow> = ingredients
        .iter()
        .enumerate()
        .map(|(i, ing)| IngredientRow {
            ing: IngredientInput {
                quantity: ing
                    .quantity
                    .map(format_quantity_for_input)
                    .unwrap_or_default(),
                unit: ing.unit.slug().to_owned(),
                name: ing.name.clone(),
            },
            row_id: format!("ing-{i}"),
            unit_options: unit_options(ing.unit.slug()),
            err_quantity: None,
            err_name: None,
        })
        .collect();

    let step_rows: Vec<StepRow> = steps
        .iter()
        .enumerate()
        .map(|(i, s)| StepRow {
            step: StepInput {
                wait_minutes: s.wait_minutes,
                text: s.text.clone(),
            },
            row_id: format!("step-{i}"),
            err_text: None,
        })
        .collect();

    // We don't store prep/cook separately yet — fall back to the sum on
    // `time_minutes` (split evenly so the inputs aren't both empty).
    let (prep, cook) = split_time_for_input(row.time_minutes.max(0) as u32);

    EditRecipePage {
        nav_items: crate::recipes::nav_items(),
        user_initial: crate::recipes::user_initial(user),
        user_name: "Jenny Rosen",
        user_email: "jenny@imkitchen.app",
        user_premium: matches!(user.role, Role::Premium),

        recipe_id: row.id.clone(),
        title: row.title.clone(),
        cuisine: row.cuisine.clone(),
        description: row.description.clone(),
        prep_minutes: prep,
        cook_minutes: cook,
        servings: row.servings.max(1) as u32,

        meal_type_options: meal_type_options(&row.meal_type),
        difficulty_options: difficulty_options(&row.difficulty),
        tag_options: tag_options(&row.tags()),

        ingredient_rows,
        step_rows,

        err: None,
        summary_err: None,
        saved: false,
        confirming,
    }
}

// ── Section POST handlers ──────────────────────────────────────────────
//
// Every handler follows the same skeleton:
//
//   1. Parse the section's form payload.
//   2. Dispatch the matching command.
//   3. On TwinSpark partial requests, return the section fragment refreshed
//      from the new state (with `saved=true` for the inline "Saved." pill).
//   4. Otherwise redirect to `/recipes/{id}/edit?awaiting=1`, reusing the
//      poll-until-projection-ready pattern from the create flow.
//
// On invalid input or aggregate error we render the section with an inline
// error (status 422 for full responses; partial keeps 200 so the swap still
// happens).

#[tracing::instrument(name = "recipes.edit.title", skip(state, body), fields(recipe_id = %id))]
pub async fn rename_section(
    user: User,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let title = first_value(&body, "title").unwrap_or_default();
    let trimmed = title.trim();
    if trimmed.is_empty() {
        let frag = EditTitleFragment {
            recipe_id: id,
            title,
            err: Some("Add a title.".into()),
            saved: false,
        };
        return error_section_response(&headers, frag);
    }

    let owner = owner_id(&user);
    let result = rename_recipe(
        RenameRecipe {
            recipe_id: id.clone(),
            owner_id: owner,
            new_title: trimmed.to_owned(),
        },
        &state.evento,
    )
    .await;

    match result {
        Ok(()) => {
            if wants_partial(&headers) {
                render(EditTitleFragment {
                    recipe_id: id,
                    title: trimmed.to_owned(),
                    err: None,
                    saved: true,
                })
            } else {
                Redirect::to(&format!("/recipes/{id}/edit?awaiting=1")).into_response()
            }
        }
        Err(err) => command_error_response(&headers, err, |msg| EditTitleFragment {
            recipe_id: id.clone(),
            title: trimmed.to_owned(),
            err: Some(msg),
            saved: false,
        }),
    }
}

#[tracing::instrument(name = "recipes.edit.categorization", skip(state, body), fields(recipe_id = %id))]
pub async fn recategorize_section(
    user: User,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let meal_type_raw = first_value(&body, "meal_type").unwrap_or_default();
    let cuisine = first_value(&body, "cuisine").unwrap_or_default();
    let meal_type = MealType::parse(&meal_type_raw).unwrap_or(MealType::Main);

    let owner = owner_id(&user);
    let result = recategorize_recipe(
        RecategorizeRecipe {
            recipe_id: id.clone(),
            owner_id: owner,
            meal_type,
            cuisine: cuisine.clone(),
            emoji: String::new(),
        },
        &state.evento,
    )
    .await;

    match result {
        Ok(()) => {
            if wants_partial(&headers) {
                render(EditCategorizationFragment {
                    recipe_id: id,
                    cuisine,
                    meal_type_options: meal_type_options(meal_type.slug()),
                    saved: true,
                })
            } else {
                Redirect::to(&format!("/recipes/{id}/edit?awaiting=1")).into_response()
            }
        }
        Err(err) => command_error_response(&headers, err, |_| EditCategorizationFragment {
            recipe_id: id.clone(),
            cuisine: cuisine.clone(),
            meal_type_options: meal_type_options(meal_type.slug()),
            saved: false,
        }),
    }
}

#[tracing::instrument(name = "recipes.edit.timing", skip(state, body), fields(recipe_id = %id))]
pub async fn retime_section(
    user: User,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let prep_minutes = first_value(&body, "prep_minutes")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    let cook_minutes = first_value(&body, "cook_minutes")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    let servings = first_value(&body, "servings")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(1)
        .max(1);
    let difficulty = first_value(&body, "difficulty").unwrap_or_else(|| "Medium".into());

    let owner = owner_id(&user);
    let result = retime_recipe(
        RetimeRecipe {
            recipe_id: id.clone(),
            owner_id: owner,
            prep_minutes,
            cook_minutes,
            servings,
            difficulty: difficulty.clone(),
        },
        &state.evento,
    )
    .await;

    let fragment_factory = |err: Option<String>, saved: bool| EditTimingFragment {
        recipe_id: id.clone(),
        prep_minutes,
        cook_minutes,
        servings,
        difficulty_options: difficulty_options(&difficulty),
        err,
        saved,
    };

    match result {
        Ok(()) => {
            if wants_partial(&headers) {
                render(fragment_factory(None, true))
            } else {
                Redirect::to(&format!("/recipes/{id}/edit?awaiting=1")).into_response()
            }
        }
        Err(err) => command_error_response(&headers, err, |msg| fragment_factory(Some(msg), false)),
    }
}

#[tracing::instrument(name = "recipes.edit.description", skip(state, body), fields(recipe_id = %id))]
pub async fn redescribe_section(
    user: User,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let description = first_value(&body, "description").unwrap_or_default();

    let owner = owner_id(&user);
    let result = redescribe_recipe(
        RedescribeRecipe {
            recipe_id: id.clone(),
            owner_id: owner,
            description: description.clone(),
        },
        &state.evento,
    )
    .await;

    match result {
        Ok(()) => {
            if wants_partial(&headers) {
                render(EditDescriptionFragment {
                    recipe_id: id,
                    description,
                    saved: true,
                })
            } else {
                Redirect::to(&format!("/recipes/{id}/edit?awaiting=1")).into_response()
            }
        }
        Err(err) => command_error_response(&headers, err, |_| EditDescriptionFragment {
            recipe_id: id.clone(),
            description: description.clone(),
            saved: false,
        }),
    }
}

#[tracing::instrument(name = "recipes.edit.tags", skip(state, body), fields(recipe_id = %id))]
pub async fn retag_section(
    user: User,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let tags: Vec<String> = all_values(&body, "tag");

    let owner = owner_id(&user);
    let result = retag_recipe(
        RetagRecipe {
            recipe_id: id.clone(),
            owner_id: owner,
            tags: tags.clone(),
        },
        &state.evento,
    )
    .await;

    match result {
        Ok(()) => {
            if wants_partial(&headers) {
                render(EditTagsFragment {
                    recipe_id: id,
                    tag_options: tag_options(&tags),
                    saved: true,
                })
            } else {
                Redirect::to(&format!("/recipes/{id}/edit?awaiting=1")).into_response()
            }
        }
        Err(err) => command_error_response(&headers, err, |_| EditTagsFragment {
            recipe_id: id.clone(),
            tag_options: tag_options(&tags),
            saved: false,
        }),
    }
}

#[tracing::instrument(name = "recipes.edit.ingredients", skip(state, body), fields(recipe_id = %id))]
pub async fn ingredients_section(
    user: User,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let inputs = parse_ingredient_inputs(&body);
    let has_any = inputs.iter().any(|i| !i.name.trim().is_empty());

    if !has_any {
        return error_section_response(
            &headers,
            EditIngredientsFragment {
                recipe_id: id,
                ingredient_rows: rows_for_ingredients(&inputs),
                summary_err: Some("Add at least one ingredient.".into()),
                saved: false,
            },
        );
    }

    let domain: Vec<DomainIngredient> = inputs
        .iter()
        .map(|i| DomainIngredient {
            name: i.name.clone(),
            quantity: parse_quantity(&i.quantity),
            unit: Unit::parse(&i.unit).unwrap_or(Unit::None),
        })
        .collect();

    let owner = owner_id(&user);
    let result = replace_ingredients(
        ReplaceIngredients {
            recipe_id: id.clone(),
            owner_id: owner,
            ingredients: domain,
        },
        &state.evento,
    )
    .await;

    match result {
        Ok(()) => {
            if wants_partial(&headers) {
                // Echo back the trimmed list (blanks dropped) so the row IDs
                // stay sequential.
                let kept: Vec<IngredientInput> = inputs
                    .into_iter()
                    .filter(|i| !i.name.trim().is_empty())
                    .collect();
                render(EditIngredientsFragment {
                    recipe_id: id,
                    ingredient_rows: rows_for_ingredients(&kept),
                    summary_err: None,
                    saved: true,
                })
            } else {
                Redirect::to(&format!("/recipes/{id}/edit?awaiting=1")).into_response()
            }
        }
        Err(err) => command_error_response(&headers, err, |msg| EditIngredientsFragment {
            recipe_id: id.clone(),
            ingredient_rows: rows_for_ingredients(&inputs),
            summary_err: Some(msg),
            saved: false,
        }),
    }
}

#[tracing::instrument(name = "recipes.edit.steps", skip(state, body), fields(recipe_id = %id))]
pub async fn steps_section(
    user: User,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let inputs = parse_step_inputs(&body);
    let has_any = inputs.iter().any(|s| !s.text.trim().is_empty());

    if !has_any {
        return error_section_response(
            &headers,
            EditStepsFragment {
                recipe_id: id,
                step_rows: rows_for_steps(&inputs),
                summary_err: Some("Add at least one step.".into()),
                saved: false,
            },
        );
    }

    let domain: Vec<DomainStep> = inputs
        .iter()
        .map(|s| DomainStep {
            wait_minutes: s.wait_minutes,
            text: s.text.clone(),
        })
        .collect();

    let owner = owner_id(&user);
    let result = replace_steps(
        ReplaceSteps {
            recipe_id: id.clone(),
            owner_id: owner,
            steps: domain,
        },
        &state.evento,
    )
    .await;

    match result {
        Ok(()) => {
            if wants_partial(&headers) {
                let kept: Vec<StepInput> = inputs
                    .into_iter()
                    .filter(|s| !s.text.trim().is_empty())
                    .collect();
                render(EditStepsFragment {
                    recipe_id: id,
                    step_rows: rows_for_steps(&kept),
                    summary_err: None,
                    saved: true,
                })
            } else {
                Redirect::to(&format!("/recipes/{id}/edit?awaiting=1")).into_response()
            }
        }
        Err(err) => command_error_response(&headers, err, |msg| EditStepsFragment {
            recipe_id: id.clone(),
            step_rows: rows_for_steps(&inputs),
            summary_err: Some(msg),
            saved: false,
        }),
    }
}

#[tracing::instrument(name = "recipes.edit.delete", skip(state), fields(recipe_id = %id))]
pub async fn delete_section(
    user: User,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let owner = owner_id(&user);
    match delete_recipe(
        DeleteRecipe {
            recipe_id: id.clone(),
            owner_id: owner,
        },
        &state.evento,
    )
    .await
    {
        // Send the user back to the library. The projection still has the
        // row for a beat; the library lists *all* the owner's recipes, so
        // the stale row will simply hang around for one polling tick — far
        // less confusing than a half-deleted detail page would be.
        Ok(()) => Redirect::to("/recipes").into_response(),
        Err(err) => {
            tracing::error!(error = %err, "delete failed");
            (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response()
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn not_found_response(user: &User) -> Response {
    use crate::recipes::{RecipeNotFoundPage, nav_items, user_initial};
    let body = RecipeNotFoundPage {
        requested_id: String::new(),
        nav_items: nav_items(),
        user_initial: user_initial(user),
        user_name: "Jenny Rosen",
        user_email: "jenny@imkitchen.app",
        user_premium: matches!(user.role, Role::Premium),
    };
    match body.render() {
        Ok(html) => (StatusCode::NOT_FOUND, Html(html)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Render a section fragment with a validation / aggregate error attached.
/// Partial requests get the section back at 200 (TwinSpark needs the swap to
/// happen even on errors); full responses get 422.
fn error_section_response<T: Template>(headers: &HeaderMap, fragment: T) -> Response {
    if wants_partial(headers) {
        render(fragment)
    } else {
        let mut resp = render(fragment);
        *resp.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
        resp
    }
}

/// Dispatch helper: build a section fragment from the aggregate error and
/// pick the right status code via `error_section_response`. `make` consumes
/// the message so the caller doesn't have to clone the strings.
fn command_error_response<T, F>(headers: &HeaderMap, err: anyhow::Error, make: F) -> Response
where
    T: Template,
    F: FnOnce(String) -> T,
{
    let msg = err.to_string();
    tracing::warn!(error = %msg, "edit command failed");
    let fragment = make(msg);
    error_section_response(headers, fragment)
}

fn render<T: Template>(tmpl: T) -> Response {
    match tmpl.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

fn first_value(body: &[u8], key: &str) -> Option<String> {
    form_urlencoded::parse(body)
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

fn all_values(body: &[u8], key: &str) -> Vec<String> {
    form_urlencoded::parse(body)
        .filter_map(|(k, v)| if k == key { Some(v.into_owned()) } else { None })
        .collect()
}

fn parse_ingredient_inputs(body: &[u8]) -> Vec<IngredientInput> {
    let mut qty: Vec<String> = Vec::new();
    let mut unit: Vec<String> = Vec::new();
    let mut name: Vec<String> = Vec::new();
    for (k, v) in form_urlencoded::parse(body) {
        match k.as_ref() {
            "ing_quantity" => qty.push(v.into_owned()),
            "ing_unit" => unit.push(v.into_owned()),
            "ing_name" => name.push(v.into_owned()),
            _ => {}
        }
    }
    let n = qty.len().max(unit.len()).max(name.len());
    (0..n)
        .map(|i| IngredientInput {
            quantity: qty.get(i).cloned().unwrap_or_default(),
            unit: unit.get(i).cloned().unwrap_or_default(),
            name: name.get(i).cloned().unwrap_or_default(),
        })
        .collect()
}

fn parse_step_inputs(body: &[u8]) -> Vec<StepInput> {
    let mut text: Vec<String> = Vec::new();
    let mut wait: Vec<u32> = Vec::new();
    for (k, v) in form_urlencoded::parse(body) {
        match k.as_ref() {
            "step_text" => text.push(v.into_owned()),
            "step_min" => wait.push(v.parse().unwrap_or(0)),
            _ => {}
        }
    }
    let n = text.len().max(wait.len());
    (0..n)
        .map(|i| StepInput {
            text: text.get(i).cloned().unwrap_or_default(),
            wait_minutes: wait.get(i).copied().unwrap_or(0),
        })
        .collect()
}

fn parse_quantity(raw: &str) -> Option<f32> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<f32>().ok().filter(|n| *n >= 0.0)
}

fn rows_for_ingredients(inputs: &[IngredientInput]) -> Vec<IngredientRow> {
    inputs
        .iter()
        .enumerate()
        .map(|(i, ing)| IngredientRow {
            ing: ing.clone(),
            row_id: format!("ing-edit-{i}-{}", next_row_serial()),
            unit_options: unit_options(&ing.unit),
            err_quantity: None,
            err_name: None,
        })
        .collect()
}

fn rows_for_steps(inputs: &[StepInput]) -> Vec<StepRow> {
    inputs
        .iter()
        .enumerate()
        .map(|(i, step)| StepRow {
            step: step.clone(),
            row_id: format!("step-edit-{i}-{}", next_row_serial()),
            err_text: None,
        })
        .collect()
}

/// Monotonic suffix so post-submit row IDs don't collide with the ones
/// rendered on the initial GET (twinspark's settling matches by `id`, and
/// reusing IDs across swaps would break focus / transitions).
fn next_row_serial() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

fn format_quantity_for_input(q: f32) -> String {
    if (q.fract()).abs() < f32::EPSILON {
        format!("{}", q as i64)
    } else {
        // Strip trailing zeros so 1.50 → "1.5" (matches what the user
        // probably typed).
        let s = format!("{q:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Best-effort split of `time_minutes` into prep + cook so the edit form
/// has reasonable defaults. The projection only stores the sum; if the user
/// keeps both inputs at these defaults the resulting `RecipeRetimed` event
/// will carry whatever they end up entering — there's no information loss
/// because we never persisted prep / cook separately.
fn split_time_for_input(total: u32) -> (u32, u32) {
    // Match the create form's defaults (15 prep, 30 cook = 45 total) when
    // we can; otherwise put it all in `cook`.
    if total == 45 {
        (15, 30)
    } else {
        (0, total)
    }
}
