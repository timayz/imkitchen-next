//! Recipes catalog — read side.
//!
//! Two response shapes:
//!  - full page → [`RecipesIndexPage`] (`recipes/index.html`)
//!  - swap body → [`RecipesGridFragment`] (`recipes/_fragment.html`).
//!    The fragment is three top-level siblings: the `#recipes-grid` section
//!    (matches `ts-target` on every trigger) plus `#view-toggle` and
//!    `#filter-chips` carrying `ts-swap-push`, so a single response refreshes
//!    the grid AND the active-state of the chrome out-of-band. Triggers also
//!    set `ts-req-history="push"` so the browser URL bar tracks state.
//!
//! Data comes from the `recipes_view` projection (see
//! `imkitchen_recipes::projection::recipes_view`).
//!
//! Owner scoping: `user.id` (the per-login ULID baked into the session
//! cookie at `POST /login`). Logging out drops the id so recipes created
//! pre-logout aren't visible after the next login — see `auth.rs` for the
//! lifecycle.

use askama::Template;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use imkitchen_recipes::{
    projection::recipes_view::{self, RecipeRow, RecipesQuery as ProjectionQuery},
    recipe::{IngredientFact, MealType, StepFact},
};
use serde::Deserialize;

use crate::{AppState, auth::User};

/// TwinSpark sets `Accept: text/html+partial` on every ts-req call. Detecting
/// this lets the handler return just the fragment block instead of the full
/// page. (TwinSpark does NOT send a custom `ts-request` header.)
const TS_PARTIAL_ACCEPT: &str = "text/html+partial";

fn wants_partial(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains(TS_PARTIAL_ACCEPT))
}

/// View model fed to the templates. Owns the row data plus pre-computed
/// helpers (`type_slug`, `type_label`, `total_ingredients`, `total_steps`)
/// that the templates rely on. Built from a [`RecipeRow`] via [`Recipe::from_row`].
#[derive(Debug, Clone)]
pub struct Recipe {
    pub id: String,
    pub title: String,
    pub kind: MealType,
    pub emoji: String,
    pub cuisine: String,
    pub time_minutes: u32,
    pub servings: u32,
    pub rating: f64,
    pub difficulty: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub ingredients: Vec<IngredientFact>,
    pub steps: Vec<StepFact>,
}

impl Recipe {
    fn from_row(row: RecipeRow) -> Self {
        let kind = MealType::parse(&row.meal_type).unwrap_or(MealType::Main);
        let description = if row.description.is_empty() {
            None
        } else {
            Some(row.description.clone())
        };
        let tags = row.tags();
        let ingredients = row.ingredients();
        let steps = row.steps();
        Self {
            id: row.id,
            title: row.title,
            kind,
            emoji: row.emoji,
            cuisine: row.cuisine,
            time_minutes: row.time_minutes.max(0) as u32,
            servings: row.servings.max(1) as u32,
            rating: row.rating,
            difficulty: row.difficulty,
            description,
            tags,
            ingredients,
            steps,
        }
    }

    // Helpers used by templates (Askama calls zero-arg methods on values).
    pub fn type_slug(&self) -> &'static str {
        self.kind.slug()
    }
    pub fn type_label(&self) -> &'static str {
        self.kind.label()
    }
    pub fn total_steps(&self) -> usize {
        self.steps.len()
    }
    pub fn total_ingredients(&self) -> usize {
        self.ingredients.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FilterId {
    #[default]
    All,
    Entree,
    Main,
    Side,
    Dessert,
}

impl FilterId {
    fn slug(self) -> &'static str {
        match self {
            FilterId::All => "all",
            FilterId::Entree => "entree",
            FilterId::Main => "main",
            FilterId::Side => "side",
            FilterId::Dessert => "dessert",
        }
    }

    fn meal_type_slug(self) -> Option<&'static str> {
        match self {
            FilterId::All => None,
            FilterId::Entree => Some("entree"),
            FilterId::Main => Some("main"),
            FilterId::Side => Some("side"),
            FilterId::Dessert => Some("dessert"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ViewId {
    #[default]
    Grid,
    List,
}

impl ViewId {
    fn slug(self) -> &'static str {
        match self {
            ViewId::Grid => "grid",
            ViewId::List => "list",
        }
    }

    /// Spoken / written form used in the status line and view-toggle labels.
    fn label(self) -> &'static str {
        match self {
            ViewId::Grid => "grid view",
            ViewId::List => "list view",
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct RecipesQuery {
    #[serde(default)]
    pub filter: FilterId,
    #[serde(default)]
    pub view: ViewId,
    #[serde(default)]
    pub q: Option<String>,
}

/// Override one query field while preserving the others, then return the URL
/// `/recipes?…` (or just `/recipes` if every field is at its default). Used by
/// filter chips, view toggle, and the search form so each one preserves the
/// other params.
///
/// `override_*` is `Some(value)` when the caller wants to change that param,
/// or `None` to keep the current request's value.
fn build_href(
    current: &RecipesQuery,
    override_filter: Option<FilterId>,
    override_view: Option<ViewId>,
    override_q: Option<Option<&str>>,
) -> String {
    let filter = override_filter.unwrap_or(current.filter);
    let view = override_view.unwrap_or(current.view);
    let q: Option<&str> = match override_q {
        Some(o) => o,
        None => current.q.as_deref(),
    };

    let mut ser = form_urlencoded::Serializer::new(String::new());
    if filter != FilterId::default() {
        ser.append_pair("filter", filter.slug());
    }
    if view != ViewId::default() {
        ser.append_pair("view", view.slug());
    }
    if let Some(needle) = q.map(str::trim).filter(|s| !s.is_empty()) {
        ser.append_pair("q", needle);
    }
    let qs = ser.finish();
    if qs.is_empty() {
        "/recipes".to_owned()
    } else {
        format!("/recipes?{qs}")
    }
}

pub struct FilterChip {
    pub label: &'static str,
    pub emoji: &'static str,
    pub active: bool,
    pub href: String,
}

pub struct ViewOption {
    pub label: &'static str,
    pub active: bool,
    pub icon_svg: &'static str,
    pub href: String,
}

pub struct NavItem {
    pub label: &'static str,
    pub hint: &'static str,
    pub href: &'static str,
    pub active: bool,
    pub icon_svg: &'static str,
}

/// Query params on `/recipes/{id}`. `awaiting=1` is set by the create-flow
/// redirect so the detail handler knows to render a poll-loop while the
/// projection catches up rather than serving a true 404. `attempt` counts
/// poll iterations so we can stop after [`ATTEMPT_LIMIT`].
#[derive(Debug, Deserialize, Default)]
pub struct DetailQuery {
    #[serde(default)]
    pub awaiting: u8,
    #[serde(default)]
    pub attempt: u8,
}

/// Roughly 5 seconds of polling at 300 ms per attempt (plus the initial
/// 300 ms before the first poll). Past this we render a terminal "still
/// working" panel and stop polling.
pub const ATTEMPT_LIMIT: u8 = 16;

// ── Templates ────────────────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "recipes/index.html")]
pub struct RecipesIndexPage {
    pub total_count: usize,
    pub filters: Vec<FilterChip>,
    pub view_options: Vec<ViewOption>,
    pub active_view: &'static str,
    pub view_label: &'static str,
    /// Hidden-input values for the search form so it preserves the current
    /// filter + view on submit.
    pub current_filter: &'static str,
    pub current_view: &'static str,
    pub current_q: String,
    pub nav_items: Vec<NavItem>,
    pub user_initial: &'static str,
    pub user_name: &'static str,
    pub user_email: &'static str,
    pub user_premium: bool,
    pub recipes: Vec<Recipe>,
}

#[derive(Template)]
#[template(path = "recipes/detail.html")]
pub struct RecipeDetailPage {
    pub recipe: Recipe,
    pub nav_items: Vec<NavItem>,
    pub user_initial: &'static str,
    pub user_name: &'static str,
    pub user_email: &'static str,
    pub user_premium: bool,
    /// `true` for the recipe's owner — gates the "Edit" link. Always `true`
    /// today because `find_for_owner` already filters by owner; reserved
    /// for a future shared-catalog view where non-owners can read but not
    /// edit.
    pub can_edit: bool,
}

#[derive(Template)]
#[template(path = "recipes/not_found.html")]
pub struct RecipeNotFoundPage {
    pub requested_id: String,
    pub nav_items: Vec<NavItem>,
    pub user_initial: &'static str,
    pub user_name: &'static str,
    pub user_email: &'static str,
    pub user_premium: bool,
}

/// Full-page loading shell rendered by the browser-side first hit on
/// `/recipes/{id}?awaiting=1`. The body of the page is the same fragment
/// (`_detail_loading.html`) that twinspark swaps in on subsequent polls.
#[derive(Template)]
#[template(path = "recipes/detail_loading.html")]
pub struct RecipeDetailLoadingPage {
    pub recipe_id: String,
    pub attempt: u8,
    pub attempt_limit: u8,
    pub nav_items: Vec<NavItem>,
    pub user_initial: &'static str,
    pub user_name: &'static str,
    pub user_email: &'static str,
    pub user_premium: bool,
}

/// Partial-only loading shell. Returned with `Accept: text/html+partial` to
/// replace the previous poll's shell, re-arming the `ts-trigger="load
/// delay:300ms"`.
#[derive(Template)]
#[template(path = "recipes/_detail_loading.html")]
pub struct RecipeDetailLoadingFragment {
    pub recipe_id: String,
    pub attempt: u8,
    pub attempt_limit: u8,
}

#[derive(Template)]
#[template(path = "recipes/_fragment.html")]
pub struct RecipesGridFragment {
    pub active_view: &'static str,
    pub view_label: &'static str,
    pub recipes: Vec<Recipe>,
    pub filters: Vec<FilterChip>,
    pub view_options: Vec<ViewOption>,
}

// ── Handler ──────────────────────────────────────────────────────────────

/// Owner-id helper. See module docs.
pub(crate) fn owner_id(user: &User) -> String {
    user.id.clone()
}

#[tracing::instrument(name = "recipes.index", skip(state, headers), fields(role = user.role.as_str()))]
pub async fn recipes_index(
    user: User,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<RecipesQuery>,
) -> Response {
    let owner = owner_id(&user);

    let query = ProjectionQuery {
        meal_type: q.filter.meal_type_slug().map(str::to_owned),
        search: q.q.clone(),
    };

    let total_count = match recipes_view::total_count(&state.read_pool, &owner).await {
        Ok(n) => n as usize,
        Err(err) => {
            tracing::error!(error = %err, "recipes_view::total_count failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response();
        }
    };

    let rows = match recipes_view::list_for_owner(&state.read_pool, &owner, &query).await {
        Ok(rs) => rs,
        Err(err) => {
            tracing::error!(error = %err, "recipes_view::list_for_owner failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response();
        }
    };
    let recipes: Vec<Recipe> = rows.into_iter().map(Recipe::from_row).collect();

    let filters = filter_chips(&q);
    let view_options = view_options(&q);

    if wants_partial(&headers) {
        // Tell TwinSpark the canonical URL to push into history. Filter chips
        // and view toggles set `ts-req-history="push"` and ts-req points at the
        // full URL — TwinSpark would push that anyway. But the search input's
        // `ts-req="/recipes"` is bare; without this header TwinSpark would
        // push `/recipes` and refreshing would lose the search.
        let canonical = build_href(&q, None, None, None);
        let mut resp = render(RecipesGridFragment {
            active_view: q.view.slug(),
            view_label: q.view.label(),
            recipes,
            filters,
            view_options,
        });
        if let Ok(value) = HeaderValue::from_str(&canonical) {
            resp.headers_mut().insert("ts-history", value);
        }
        return resp;
    }

    let chrome = chrome_for(&user);
    render(RecipesIndexPage {
        total_count,
        filters,
        view_options,
        active_view: q.view.slug(),
        view_label: q.view.label(),
        current_filter: q.filter.slug(),
        current_view: q.view.slug(),
        current_q: q.q.clone().unwrap_or_default(),
        nav_items: chrome.nav_items,
        user_initial: chrome.user_initial,
        user_name: chrome.user_name,
        user_email: chrome.user_email,
        user_premium: chrome.user_premium,
        recipes,
    })
}

// ── Recipe detail handler ────────────────────────────────────────────────

#[tracing::instrument(name = "recipes.detail", skip(state, headers), fields(recipe_id = %id, role = user.role.as_str(), awaiting = q.awaiting, attempt = q.attempt))]
pub async fn recipe_detail(
    user: User,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<DetailQuery>,
) -> Response {
    let owner = owner_id(&user);

    let row = match recipes_view::find_for_owner(&state.read_pool, &owner, &id).await {
        Ok(r) => r,
        Err(err) => {
            tracing::error!(error = %err, "recipes_view::find_for_owner failed");
            return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response();
        }
    };

    if let Some(row) = row {
        // Projection is ready. If we're in the middle of a poll loop, kick
        // the browser to the clean URL via `ts-location` so the user lands
        // on `/recipes/{id}` without the `?awaiting=...` query string.
        if q.awaiting > 0 && wants_partial(&headers) {
            let mut resp = (StatusCode::OK, "").into_response();
            if let Ok(value) = HeaderValue::from_str(&format!("/recipes/{}", row.id)) {
                resp.headers_mut().insert("ts-location", value);
            }
            return resp;
        }

        let chrome = chrome_for(&user);
        return render(RecipeDetailPage {
            recipe: Recipe::from_row(row),
            nav_items: chrome.nav_items,
            user_initial: chrome.user_initial,
            user_name: chrome.user_name,
            user_email: chrome.user_email,
            user_premium: chrome.user_premium,
            // `find_for_owner` already scoped the read by owner, so anyone
            // reaching this branch owns the recipe.
            can_edit: true,
        });
    }

    // Row not (yet) in the projection.
    if q.awaiting > 0 {
        // Create-redirect lane: keep polling. The shell carries the next
        // attempt count; when it crosses ATTEMPT_LIMIT we render a terminal
        // "still working" panel with no trigger and the poll loop ends.
        let attempt = q.attempt.min(ATTEMPT_LIMIT);
        if wants_partial(&headers) {
            return render(RecipeDetailLoadingFragment {
                recipe_id: id,
                attempt,
                attempt_limit: ATTEMPT_LIMIT,
            });
        }
        let chrome = chrome_for(&user);
        return render(RecipeDetailLoadingPage {
            recipe_id: id,
            attempt,
            attempt_limit: ATTEMPT_LIMIT,
            nav_items: chrome.nav_items,
            user_initial: chrome.user_initial,
            user_name: chrome.user_name,
            user_email: chrome.user_email,
            user_premium: chrome.user_premium,
        });
    }

    // Genuine 404: no `?awaiting=1`, so this isn't a freshly-created recipe
    // catching up — the recipe doesn't exist (or isn't ours).
    let chrome = chrome_for(&user);
    let body = RecipeNotFoundPage {
        requested_id: id,
        nav_items: chrome.nav_items,
        user_initial: chrome.user_initial,
        user_name: chrome.user_name,
        user_email: chrome.user_email,
        user_premium: chrome.user_premium,
    };
    match body.render() {
        Ok(html) => (StatusCode::NOT_FOUND, Html(html)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub(crate) fn user_initial(user: &User) -> &'static str {
    match user.role {
        crate::auth::Role::User => "U",
        crate::auth::Role::Chef => "C",
        crate::auth::Role::Premium => "P",
    }
}

/// Inputs the side rail + mobile bottom nav need on every recipes page.
/// Both [`RecipesIndexPage`] and [`RecipeDetailPage`] consume this.
struct Chrome {
    nav_items: Vec<NavItem>,
    user_initial: &'static str,
    user_name: &'static str,
    user_email: &'static str,
    user_premium: bool,
}

fn chrome_for(user: &User) -> Chrome {
    Chrome {
        nav_items: nav_items(),
        user_initial: user_initial(user),
        user_name: "Jenny Rosen",
        user_email: "jenny@imkitchen.app",
        user_premium: matches!(user.role, crate::auth::Role::Premium),
    }
}

fn render<T: Template>(tmpl: T) -> Response {
    match tmpl.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// ── Static chrome ───────────────────────────────────────────────────────

const ICON_BOOK: &str = r##"<svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 5a2 2 0 012-2h13v16H6a2 2 0 00-2 2V5z"/><path d="M4 19a2 2 0 012-2h13"/></svg>"##;
const ICON_CHEF: &str = r##"<svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 14v5a1 1 0 001 1h10a1 1 0 001-1v-5"/><path d="M5 14a3 3 0 01-.5-5.9A4 4 0 0112 5a4 4 0 017.5 3.1A3 3 0 0119 14H5z"/></svg>"##;
const ICON_CALENDAR: &str = r##"<svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="5" width="18" height="16" rx="2"/><path d="M3 9h18M8 3v4M16 3v4"/></svg>"##;
const ICON_CART: &str = r##"<svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="9" cy="20" r="1.5"/><circle cx="18" cy="20" r="1.5"/><path d="M3 4h2l2.7 11.3a1 1 0 001 .7h9.6a1 1 0 001-.7L21 8H6"/></svg>"##;
const ICON_SETTINGS: &str = r##"<svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 00.3 1.8l.1.1a2 2 0 11-2.8 2.8l-.1-.1a1.7 1.7 0 00-1.8-.3 1.7 1.7 0 00-1 1.5V21a2 2 0 11-4 0v-.1a1.7 1.7 0 00-1.1-1.5 1.7 1.7 0 00-1.8.3l-.1.1a2 2 0 11-2.8-2.8l.1-.1a1.7 1.7 0 00.3-1.8 1.7 1.7 0 00-1.5-1H3a2 2 0 110-4h.1a1.7 1.7 0 001.5-1.1 1.7 1.7 0 00-.3-1.8l-.1-.1a2 2 0 112.8-2.8l.1.1a1.7 1.7 0 001.8.3H9a1.7 1.7 0 001-1.5V3a2 2 0 114 0v.1a1.7 1.7 0 001 1.5 1.7 1.7 0 001.8-.3l.1-.1a2 2 0 112.8 2.8l-.1.1a1.7 1.7 0 00-.3 1.8V9a1.7 1.7 0 001.5 1H21a2 2 0 110 4h-.1a1.7 1.7 0 00-1.5 1z"/></svg>"##;

const ICON_GRID: &str = r##"<svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/></svg>"##;
const ICON_LIST: &str = r##"<svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01"/></svg>"##;

pub(crate) fn nav_items() -> Vec<NavItem> {
    vec![
        NavItem {
            label: "Kitchen",
            hint: "Today & cooking",
            href: "/",
            active: false,
            icon_svg: ICON_CHEF,
        },
        NavItem {
            label: "Menu",
            hint: "Weekly plan",
            href: "/menu",
            active: false,
            icon_svg: ICON_CALENDAR,
        },
        NavItem {
            label: "Recipes",
            hint: "Your library",
            href: "/recipes",
            active: true,
            icon_svg: ICON_BOOK,
        },
        NavItem {
            label: "Shop",
            hint: "Groceries & route",
            href: "/shop",
            active: false,
            icon_svg: ICON_CART,
        },
        NavItem {
            label: "Settings",
            hint: "Account & billing",
            href: "/settings",
            active: false,
            icon_svg: ICON_SETTINGS,
        },
    ]
}

fn filter_chips(current: &RecipesQuery) -> Vec<FilterChip> {
    [
        (FilterId::All, "All", "🍴"),
        (FilterId::Entree, "Starters", "🥗"),
        (FilterId::Main, "Mains", "🍛"),
        (FilterId::Side, "Sides", "🥖"),
        (FilterId::Dessert, "Desserts", "🍰"),
    ]
    .into_iter()
    .map(|(id, label, emoji)| FilterChip {
        label,
        emoji,
        active: id == current.filter,
        href: build_href(current, Some(id), None, None),
    })
    .collect()
}

fn view_options(current: &RecipesQuery) -> Vec<ViewOption> {
    vec![
        ViewOption {
            label: "Grid",
            active: current.view == ViewId::Grid,
            icon_svg: ICON_GRID,
            href: build_href(current, None, Some(ViewId::Grid), None),
        },
        ViewOption {
            label: "List",
            active: current.view == ViewId::List,
            icon_svg: ICON_LIST,
            href: build_href(current, None, Some(ViewId::List), None),
        },
    ]
}
