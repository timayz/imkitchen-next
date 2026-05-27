//! Import-recipe flow.
//!
//! Single endpoint, three stages selected by query/form param:
//!
//!  GET  /recipes/import?stage=upload|preview|done   → render that stage
//!  POST /recipes/import                             → advance via `next_stage`
//!
//! Sharing one URL across stages keeps the chrome (top bar, breadcrumb, live
//! region) stable for AT users and makes link-sharing predictable.
//!
//! TODO list (all the real work is stubbed):
//!  - URL source: fetch the page server-side and run schema.org / JSON-LD
//!    extraction.
//!  - Photo source: handoff to an OCR pipeline.
//!  - Text source: run the same parser used for URL once it lands.
//!  - JSON file source: accept multipart upload, parse, validate.
//!  - Persistence on "done": dispatch evento commands to actually create the
//!    selected recipes.

use askama::Template;
use axum::{
    body::Bytes,
    extract::Query,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use serde::Deserialize;

use crate::{
    auth::{Role, User},
    recipes::NavItem,
};

// ── Stage model ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    #[default]
    Upload,
    Preview,
    Done,
}

impl Stage {
    fn slug(self) -> &'static str {
        match self {
            Stage::Upload => "upload",
            Stage::Preview => "preview",
            Stage::Done => "done",
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct ImportQuery {
    #[serde(default)]
    pub stage: Stage,
}

// ── Display rows ────────────────────────────────────────────────────────

pub struct StepIndicator {
    pub label: &'static str,
    pub active: bool,
    pub done: bool,
}

fn step_indicators(current: Stage) -> Vec<StepIndicator> {
    let order = [
        (Stage::Upload, "Source"),
        (Stage::Preview, "Review"),
        (Stage::Done, "Done"),
    ];
    let cur_idx = order.iter().position(|(s, _)| *s == current).unwrap_or(0);
    order
        .iter()
        .enumerate()
        .map(|(i, (s, label))| StepIndicator {
            label,
            active: *s == current,
            done: i < cur_idx,
        })
        .collect()
}

pub struct SourceOption {
    pub id: &'static str,
    pub name: &'static str,
    pub sub: &'static str,
    pub emoji: &'static str,
}

fn source_options() -> Vec<SourceOption> {
    vec![
        SourceOption {
            id: "url",
            name: "Paste a recipe URL",
            sub: "We parse most blogs and food sites",
            emoji: "🌐",
        },
        SourceOption {
            id: "photo",
            name: "Scan a cookbook page",
            sub: "Photo → OCR → structured recipe",
            emoji: "📷",
        },
        SourceOption {
            id: "text",
            name: "Paste plain text",
            sub: "Works with any plain-text recipe",
            emoji: "📋",
        },
        SourceOption {
            id: "share",
            name: "From a friend's share link",
            sub: "imkitchen://share/…",
            emoji: "🤝",
        },
    ]
}

/// One parsed-recipe candidate shown in the preview stage. Real data will
/// come from the upstream parsers; for now we ship a fixed seed.
pub struct ParsedRecipe {
    pub id: &'static str,
    pub title: &'static str,
    pub emoji: &'static str,
    pub type_slug: &'static str,
    pub type_label: &'static str,
    pub ingredient_count: u32,
    pub step_count: u32,
    pub warn: Option<&'static str>,
    pub broken: bool,
    pub selected: bool,
}

pub struct ImportStat {
    pub label: &'static str,
    pub count: u32,
}

// ── Templates ───────────────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "recipes/import.html")]
pub struct ImportPage {
    // Chrome
    pub nav_items: Vec<NavItem>,
    pub user_initial: &'static str,
    pub user_name: &'static str,
    pub user_email: &'static str,
    pub user_premium: bool,

    pub stage: &'static str,
    pub steps: Vec<StepIndicator>,

    // Upload-stage data
    pub source_options: Vec<SourceOption>,

    // Preview-stage data
    pub source_label: &'static str,
    pub parsed: Vec<ParsedRecipe>,
    pub valid_count: usize,
    pub warning_count: usize,

    // Done-stage data
    pub imported_count: u32,
    pub stats: Vec<ImportStat>,
}

// ── Handlers ────────────────────────────────────────────────────────────

pub async fn import_page(user: User, Query(q): Query<ImportQuery>) -> Response {
    render(build_page(&user, q.stage))
}

pub async fn import_submit(user: User, body: Bytes) -> Response {
    let next = parse_next_stage(&body).unwrap_or(Stage::Preview);

    // TODO: dispatch the actual work here. For `Preview` we'd kick off the
    // parser for the chosen source; for `Done` we'd commit the picked recipes.
    // For now both transitions are pure UI advancement.

    render(build_page(&user, next))
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn build_page(user: &User, stage: Stage) -> ImportPage {
    let parsed = stub_parsed();
    let valid_count = parsed.iter().filter(|r| !r.broken).count();
    let warning_count = parsed.iter().filter(|r| r.warn.is_some()).count();

    ImportPage {
        nav_items: crate::recipes::nav_items(),
        user_initial: crate::recipes::user_initial(user),
        user_name: "Jenny Rosen",
        user_email: "jenny@imkitchen.app",
        user_premium: matches!(user.role, Role::Premium),

        stage: stage.slug(),
        steps: step_indicators(stage),

        source_options: source_options(),

        source_label: stub_source_label(),
        parsed,
        valid_count,
        warning_count,

        imported_count: stub_imported_count(),
        stats: stub_stats(),
    }
}

fn parse_next_stage(body: &[u8]) -> Option<Stage> {
    for (key, value) in form_urlencoded::parse(body) {
        if key == "next_stage" {
            return match value.as_ref() {
                "upload" => Some(Stage::Upload),
                "preview" => Some(Stage::Preview),
                "done" => Some(Stage::Done),
                _ => None,
            };
        }
    }
    None
}

// ── Stub data (replace with real parser output) ─────────────────────────

// TODO: real file name + size come from the upload step. Hard-coded here so the
// preview screen has something to render.
fn stub_source_label() -> &'static str {
    "grandmas-recipes.json · 12.4 KB"
}

// TODO: replace with output of the active parser. The shape (warn, broken,
// selected) mirrors what the preview UI needs.
fn stub_parsed() -> Vec<ParsedRecipe> {
    vec![
        ParsedRecipe {
            id: "i1",
            title: "Grandma's Pot Roast",
            emoji: "🥩",
            type_slug: "main",
            type_label: "Main",
            ingredient_count: 9,
            step_count: 6,
            warn: None,
            broken: false,
            selected: true,
        },
        ParsedRecipe {
            id: "i2",
            title: "Green Bean Casserole",
            emoji: "🥗",
            type_slug: "side",
            type_label: "Side",
            ingredient_count: 7,
            step_count: 5,
            warn: None,
            broken: false,
            selected: true,
        },
        ParsedRecipe {
            id: "i3",
            title: "Buttermilk Biscuits",
            emoji: "🥐",
            type_slug: "side",
            type_label: "Side",
            ingredient_count: 6,
            step_count: 4,
            warn: None,
            broken: false,
            selected: true,
        },
        ParsedRecipe {
            id: "i4",
            title: "Apple Pie",
            emoji: "🥧",
            type_slug: "dessert",
            type_label: "Dessert",
            ingredient_count: 10,
            step_count: 8,
            warn: None,
            broken: false,
            selected: true,
        },
        ParsedRecipe {
            id: "i5",
            title: "Pecan Bars",
            emoji: "🍪",
            type_slug: "dessert",
            type_label: "Dessert",
            ingredient_count: 8,
            step_count: 5,
            warn: Some("Similar to existing \"Pecan Squares\""),
            broken: false,
            selected: false,
        },
        ParsedRecipe {
            id: "i6",
            title: "Deviled Eggs",
            emoji: "🥚",
            type_slug: "entree",
            type_label: "Starter",
            ingredient_count: 6,
            step_count: 3,
            warn: None,
            broken: false,
            selected: true,
        },
        ParsedRecipe {
            id: "i7",
            title: "Sweet Potato Mash",
            emoji: "🍠",
            type_slug: "side",
            type_label: "Side",
            ingredient_count: 5,
            step_count: 4,
            warn: None,
            broken: false,
            selected: true,
        },
        ParsedRecipe {
            id: "i8",
            title: "Untitled recipe",
            emoji: "🍽️",
            type_slug: "main",
            type_label: "Main",
            ingredient_count: 0,
            step_count: 0,
            warn: Some("Missing title and steps — can't import"),
            broken: true,
            selected: false,
        },
    ]
}

// TODO: count comes from the POST body's `pick` values + the persistence step.
fn stub_imported_count() -> u32 {
    6
}

fn stub_stats() -> Vec<ImportStat> {
    vec![
        ImportStat {
            label: "Imported",
            count: 6,
        },
        ImportStat {
            label: "Skipped",
            count: 1,
        },
        ImportStat {
            label: "Duplicate",
            count: 1,
        },
    ]
}

fn render<T: Template>(tmpl: T) -> Response {
    match tmpl.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
