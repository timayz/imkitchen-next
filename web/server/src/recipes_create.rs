//! Create-recipe page.
//!
//! GET /recipes/new                     → empty form
//! POST /recipes/new                    → parse + validate; redirect on success
//! GET /recipes/new/ingredient-row      → one blank ingredient row (TwinSpark)
//! GET /recipes/new/step-row            → one blank step row (TwinSpark)
//!
//! TODO: persistence is stubbed — on success we 303 to `/recipes` without
//! writing anything. Replace with a write-side command once evento read/write
//! models for recipes exist.
//!
//! Form encoding: standard `application/x-www-form-urlencoded`. Ingredient and
//! step rows are sent as repeated keys (`ing_qty=…&ing_name=…&ing_qty=…`); we
//! parse the body manually with `form_urlencoded` because `axum::Form` (and the
//! underlying `serde_urlencoded`) does not accept repeated keys for `Vec<T>`.

use std::sync::atomic::{AtomicU64, Ordering};

use askama::Template;
use axum::{
    body::Bytes,
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
};

use crate::{
    auth::{Role, User},
    recipes::NavItem,
};

// ── Static option sets ──────────────────────────────────────────────────

#[derive(Debug)]
pub struct MealTypeOption {
    pub slug: &'static str,
    pub label: &'static str,
    pub emoji: &'static str,
    pub active: bool,
}

#[derive(Debug)]
pub struct DifficultyOption {
    pub value: &'static str,
    pub label: &'static str,
    pub active: bool,
}

#[derive(Debug)]
pub struct TagOption {
    pub value: &'static str,
    pub active: bool,
}

const MEAL_TYPE_VARIANTS: &[(&str, &str, &str)] = &[
    ("entree", "Starter", "🥗"),
    ("main", "Main", "🍛"),
    ("side", "Side", "🥖"),
    ("dessert", "Dessert", "🍰"),
];

const DIFFICULTY_VARIANTS: &[&str] = &["Easy", "Medium", "Hard"];

const TAG_VARIANTS: &[&str] = &[
    "Vegetarian",
    "Vegan",
    "Gluten-free",
    "Dairy-free",
    "Nut-free",
    "Low-carb",
    "High-protein",
    "Make-ahead",
    "One-pot",
    "Spicy",
];

fn meal_type_options(active_slug: &str) -> Vec<MealTypeOption> {
    MEAL_TYPE_VARIANTS
        .iter()
        .map(|(slug, label, emoji)| MealTypeOption {
            slug,
            label,
            emoji,
            active: *slug == active_slug,
        })
        .collect()
}

fn difficulty_options(active: &str) -> Vec<DifficultyOption> {
    DIFFICULTY_VARIANTS
        .iter()
        .map(|label| DifficultyOption {
            value: label,
            label,
            active: *label == active,
        })
        .collect()
}

fn tag_options(active_tags: &[String]) -> Vec<TagOption> {
    TAG_VARIANTS
        .iter()
        .map(|t| TagOption {
            value: t,
            active: active_tags.iter().any(|s| s == t),
        })
        .collect()
}

// ── Form models ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct IngredientInput {
    pub qty: String,
    pub name: String,
}

#[derive(Debug, Clone, Default)]
pub struct StepInput {
    pub minutes: u32,
    pub text: String,
}

/// One ingredient row in the rendered form (echoed back on validation failure
/// so the user doesn't lose typed input).
pub struct IngredientRow {
    pub ing: IngredientInput,
    pub row_id: String,
    pub err_qty: Option<String>,
    pub err_name: Option<String>,
}

pub struct StepRow {
    pub step: StepInput,
    pub row_id: String,
    pub err_text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreateRecipeForm {
    pub title: String,
    pub meal_type: String,
    pub cuisine: String,
    pub prep_minutes: u32,
    pub cook_minutes: u32,
    pub servings: u32,
    pub difficulty: String,
    pub description: String,
    pub tags: Vec<String>,
    pub ingredients: Vec<IngredientInput>,
    pub steps: Vec<StepInput>,
}

impl CreateRecipeForm {
    /// Starter values shown on the empty GET. Mirrors the mockup's defaults so
    /// the screenshot matches what a tester sees.
    fn starter() -> Self {
        Self {
            title: String::new(),
            meal_type: "main".to_owned(),
            cuisine: String::new(),
            prep_minutes: 15,
            cook_minutes: 30,
            servings: 4,
            difficulty: "Medium".to_owned(),
            description: String::new(),
            tags: Vec::new(),
            ingredients: vec![IngredientInput::default(); 3],
            steps: vec![StepInput::default(); 2],
        }
    }
}

#[derive(Default)]
struct FieldErrors {
    title: Option<String>,
    ingredient: Vec<(Option<String>, Option<String>)>,
    step: Vec<Option<String>>,
    summary: Vec<String>,
}

impl FieldErrors {
    fn is_empty(&self) -> bool {
        self.summary.is_empty()
    }
}

fn validate(form: &CreateRecipeForm) -> FieldErrors {
    let mut errs = FieldErrors {
        ingredient: vec![(None, None); form.ingredients.len()],
        step: vec![None; form.steps.len()],
        ..Default::default()
    };

    if form.title.trim().is_empty() {
        errs.title = Some("A title is required.".to_owned());
        errs.summary.push("Add a title.".to_owned());
    }

    let has_any_ingredient = form
        .ingredients
        .iter()
        .any(|i| !i.qty.trim().is_empty() || !i.name.trim().is_empty());
    if !has_any_ingredient {
        errs.summary.push("Add at least one ingredient.".to_owned());
    } else {
        // Per-row: if name is filled but qty is missing (or vice versa), flag it.
        for (i, ing) in form.ingredients.iter().enumerate() {
            let qty_empty = ing.qty.trim().is_empty();
            let name_empty = ing.name.trim().is_empty();
            if name_empty && !qty_empty {
                errs.ingredient[i].1 = Some("Name is required.".to_owned());
            }
            if qty_empty && !name_empty {
                errs.ingredient[i].0 = Some("Qty is required.".to_owned());
            }
        }
    }

    let has_any_step = form.steps.iter().any(|s| !s.text.trim().is_empty());
    if !has_any_step {
        errs.summary.push("Add at least one step.".to_owned());
    }

    errs
}

// ── Page templates ──────────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "recipes/create.html")]
pub struct CreateRecipePage {
    // Chrome
    pub nav_items: Vec<NavItem>,
    pub user_initial: &'static str,
    pub user_name: &'static str,
    pub user_email: &'static str,
    pub user_premium: bool,

    // Field values (echoed back on validation failure).
    pub title: String,
    pub cuisine: String,
    pub description: String,
    pub prep_minutes: u32,
    pub cook_minutes: u32,
    pub servings: u32,

    // Option lists with `.active` flag for the currently-selected choice.
    pub meal_type_options: Vec<MealTypeOption>,
    pub difficulty_options: Vec<DifficultyOption>,
    pub tag_options: Vec<TagOption>,

    // Repeated rows + per-row errors.
    pub ingredient_rows: Vec<IngredientRow>,
    pub step_rows: Vec<StepRow>,

    // Top-level field error + summary errors.
    pub err_title: Option<String>,
    pub form_errors: Vec<String>,
}

#[derive(Template)]
#[template(path = "recipes/_ing_row.html")]
pub struct IngredientRowFragment {
    pub ing: IngredientInput,
    pub row_id: String,
    pub err_qty: Option<String>,
    pub err_name: Option<String>,
}

#[derive(Template)]
#[template(path = "recipes/_step_row.html")]
pub struct StepRowFragment {
    pub step: StepInput,
    pub row_id: String,
    pub err_text: Option<String>,
}

// ── Handlers ────────────────────────────────────────────────────────────

pub async fn create_form(user: User) -> Response {
    let page = build_page(&user, CreateRecipeForm::starter(), FieldErrors::default());
    render(page)
}

pub async fn create_submit(user: User, body: Bytes) -> Response {
    let form = parse_form(&body);
    let errs = validate(&form);

    if errs.is_empty() {
        // TODO: dispatch evento command to create the recipe. For now redirect
        // straight to the library — the new entry won't actually appear there.
        return Redirect::to("/recipes").into_response();
    }

    let mut resp = render(build_page(&user, form, errs));
    *resp.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
    resp
}

pub async fn ingredient_row_fragment(_user: User) -> Response {
    let frag = IngredientRowFragment {
        ing: IngredientInput::default(),
        row_id: next_row_id("ing"),
        err_qty: None,
        err_name: None,
    };
    render(frag)
}

pub async fn step_row_fragment(_user: User) -> Response {
    let frag = StepRowFragment {
        step: StepInput::default(),
        row_id: next_row_id("step"),
        err_text: None,
    };
    render(frag)
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn build_page(user: &User, form: CreateRecipeForm, errs: FieldErrors) -> CreateRecipePage {
    let ingredient_rows: Vec<IngredientRow> = form
        .ingredients
        .into_iter()
        .enumerate()
        .map(|(i, ing)| {
            let (err_qty, err_name) = errs.ingredient.get(i).cloned().unwrap_or((None, None));
            IngredientRow {
                ing,
                row_id: format!("ing-{i}"),
                err_qty,
                err_name,
            }
        })
        .collect();

    let step_rows: Vec<StepRow> = form
        .steps
        .into_iter()
        .enumerate()
        .map(|(i, step)| StepRow {
            step,
            row_id: format!("step-{i}"),
            err_text: errs.step.get(i).cloned().flatten(),
        })
        .collect();

    CreateRecipePage {
        nav_items: crate::recipes::nav_items(),
        user_initial: crate::recipes::user_initial(user),
        user_name: "Jenny Rosen",
        user_email: "jenny@imkitchen.app",
        user_premium: matches!(user.role, Role::Premium),

        title: form.title,
        cuisine: form.cuisine,
        description: form.description,
        prep_minutes: form.prep_minutes,
        cook_minutes: form.cook_minutes,
        servings: form.servings,

        meal_type_options: meal_type_options(&form.meal_type),
        difficulty_options: difficulty_options(&form.difficulty),
        tag_options: tag_options(&form.tags),

        ingredient_rows,
        step_rows,

        err_title: errs.title,
        form_errors: errs.summary,
    }
}

/// Pulls one scalar field (last write wins, matching browser form behaviour)
/// and the parallel ingredient/step arrays out of the urlencoded body.
fn parse_form(body: &[u8]) -> CreateRecipeForm {
    let mut title = String::new();
    let mut meal_type = "main".to_owned();
    let mut cuisine = String::new();
    let mut description = String::new();
    let mut difficulty = "Medium".to_owned();
    let mut prep_minutes: u32 = 0;
    let mut cook_minutes: u32 = 0;
    let mut servings: u32 = 1;
    let mut tags: Vec<String> = Vec::new();

    let mut ing_qty: Vec<String> = Vec::new();
    let mut ing_name: Vec<String> = Vec::new();
    let mut step_text: Vec<String> = Vec::new();
    let mut step_min: Vec<u32> = Vec::new();

    for (key, value) in form_urlencoded::parse(body) {
        match key.as_ref() {
            "title" => title = value.into_owned(),
            "meal_type" => meal_type = value.into_owned(),
            "cuisine" => cuisine = value.into_owned(),
            "description" => description = value.into_owned(),
            "difficulty" => difficulty = value.into_owned(),
            "prep_minutes" => prep_minutes = value.parse().unwrap_or(0),
            "cook_minutes" => cook_minutes = value.parse().unwrap_or(0),
            "servings" => servings = value.parse().unwrap_or(1).max(1),
            "tag" => tags.push(value.into_owned()),
            "ing_qty" => ing_qty.push(value.into_owned()),
            "ing_name" => ing_name.push(value.into_owned()),
            "step_text" => step_text.push(value.into_owned()),
            "step_min" => step_min.push(value.parse().unwrap_or(0)),
            _ => {}
        }
    }

    // Zip the parallel ingredient arrays. They should always match — the row
    // template emits qty and name together — but be defensive against the user
    // hand-crafting a body or a JS error dropping one side.
    let ing_count = ing_qty.len().max(ing_name.len());
    let ingredients: Vec<IngredientInput> = (0..ing_count)
        .map(|i| IngredientInput {
            qty: ing_qty.get(i).cloned().unwrap_or_default(),
            name: ing_name.get(i).cloned().unwrap_or_default(),
        })
        .collect();

    let step_count = step_text.len().max(step_min.len());
    let steps: Vec<StepInput> = (0..step_count)
        .map(|i| StepInput {
            text: step_text.get(i).cloned().unwrap_or_default(),
            minutes: step_min.get(i).copied().unwrap_or(0),
        })
        .collect();

    CreateRecipeForm {
        title,
        meal_type,
        cuisine,
        prep_minutes,
        cook_minutes,
        servings,
        difficulty,
        description,
        tags,
        ingredients,
        steps,
    }
}

/// Monotonic suffix so client-side-added rows get unique IDs across the
/// lifetime of the process. Sufficient for label/aria-describedby uniqueness;
/// not security-sensitive.
fn next_row_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-new-{n}")
}

fn render<T: Template>(tmpl: T) -> Response {
    match tmpl.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
