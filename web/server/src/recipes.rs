//! Recipes library — the shared catalog users browse to add recipes to their
//! meal plan. UI integration only: data is seeded in [`fake_recipes`]; swap to
//! a real query (probably evento-backed) once the read side exists.
//!
//! Two response shapes:
//!  - full page → [`RecipesIndexPage`] (`recipes/index.html`)
//!  - swap body → [`RecipesGridFragment`] (`recipes/_fragment.html`).
//!    The fragment is three top-level siblings: the `#recipes-grid` section
//!    (matches `ts-target` on every trigger) plus `#view-toggle` and
//!    `#filter-chips` carrying `ts-swap-push`, so a single response refreshes
//!    the grid AND the active-state of the chrome out-of-band. Triggers also
//!    set `ts-req-history="push"` so the browser URL bar tracks state.

use askama::Template;
use axum::{
    extract::{Path, Query},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use serde::Deserialize;

use crate::auth::User;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MealType {
    Entree,
    Main,
    Side,
    Dessert,
}

impl MealType {
    /// Lowercase slug used in URLs and as the `type-*` Tailwind color suffix.
    pub fn slug(self) -> &'static str {
        match self {
            MealType::Entree => "entree",
            MealType::Main => "main",
            MealType::Side => "side",
            MealType::Dessert => "dessert",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MealType::Entree => "Starter",
            MealType::Main => "Main",
            MealType::Side => "Side",
            MealType::Dessert => "Dessert",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Ingredient {
    pub name: &'static str,
    pub qty: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct Step {
    /// Active minutes for this step (informational; not a timer).
    pub minutes: u32,
    pub text: &'static str,
}

#[derive(Debug, Clone)]
pub struct Recipe {
    pub id: &'static str,
    pub title: &'static str,
    pub kind: MealType,
    pub emoji: &'static str,
    pub cuisine: &'static str,
    pub time_minutes: u32,
    pub servings: u32,
    pub rating: f32,
    /// "Easy" / "Medium" / "Hard" — shown in the detail meta strip.
    pub difficulty: &'static str,
    /// Hero blurb on the detail page. Missing for the recipes the mockup
    /// didn't enrich; the template hides the paragraph when empty.
    pub description: Option<&'static str>,
    /// Display-only tags shown as pills next to the meal-type chip.
    pub tags: &'static [&'static str],
    /// Empty when the seed didn't include ingredients; the detail template
    /// renders a "we don't have the ingredients for this one yet" notice.
    pub ingredients: &'static [Ingredient],
    /// Empty when the seed didn't include steps; the method section is hidden
    /// in that case.
    pub steps: &'static [Step],
}

impl Recipe {
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

    fn matches(self, kind: MealType) -> bool {
        match self {
            FilterId::All => true,
            FilterId::Entree => kind == MealType::Entree,
            FilterId::Main => kind == MealType::Main,
            FilterId::Side => kind == MealType::Side,
            FilterId::Dessert => kind == MealType::Dessert,
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

pub async fn recipes_index(
    user: User,
    headers: HeaderMap,
    Query(q): Query<RecipesQuery>,
) -> Response {
    let all = fake_recipes();
    let total_count = all.len();

    let search = q.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let recipes: Vec<Recipe> = all
        .into_iter()
        .filter(|r| q.filter.matches(r.kind))
        .filter(|r| match search {
            None => true,
            Some(needle) => matches_search(r, needle),
        })
        .collect();

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

pub async fn recipe_detail(user: User, Path(id): Path<String>) -> Response {
    let Some(recipe) = fake_recipes().into_iter().find(|r| r.id == id) else {
        let chrome = chrome_for(&user);
        let body = RecipeNotFoundPage {
            requested_id: id,
            nav_items: chrome.nav_items,
            user_initial: chrome.user_initial,
            user_name: chrome.user_name,
            user_email: chrome.user_email,
            user_premium: chrome.user_premium,
        };
        return match body.render() {
            Ok(html) => (StatusCode::NOT_FOUND, Html(html)).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        };
    };

    let chrome = chrome_for(&user);
    render(RecipeDetailPage {
        recipe,
        nav_items: chrome.nav_items,
        user_initial: chrome.user_initial,
        user_name: chrome.user_name,
        user_email: chrome.user_email,
        user_premium: chrome.user_premium,
    })
}

fn matches_search(r: &Recipe, needle: &str) -> bool {
    let n = needle.to_ascii_lowercase();
    r.title.to_ascii_lowercase().contains(&n) || r.cuisine.to_ascii_lowercase().contains(&n)
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

// ── Static data (replace with read-model queries) ───────────────────────

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

/// Seed catalog mirroring the mockup. Replace once the recipes read model
/// exists.
fn fake_recipes() -> Vec<Recipe> {
    // Ingredient / step tables for the two recipes the mockup enriches in
    // detail (r1 + r6). Other recipes ship empty slices and the detail
    // template handles the missing-content case.
    const R1_INGREDIENTS: &[Ingredient] = &[
        Ingredient {
            name: "Chicken pieces",
            qty: "500 g",
        },
        Ingredient {
            name: "Long-grain rice",
            qty: "2 cups",
        },
        Ingredient {
            name: "Bell peppers",
            qty: "2",
        },
        Ingredient {
            name: "Yellow onion",
            qty: "1 large",
        },
        Ingredient {
            name: "Chicken stock",
            qty: "700 ml",
        },
        Ingredient {
            name: "Saffron",
            qty: "1 pinch",
        },
        Ingredient {
            name: "Garlic",
            qty: "4 cloves",
        },
        Ingredient {
            name: "Olive oil",
            qty: "2 tbsp",
        },
    ];
    const R1_STEPS: &[Step] = &[
        Step {
            minutes: 10,
            text: "Season chicken with salt, paprika, and cumin. Brown skin-side down in a wide pan until deeply golden.",
        },
        Step {
            minutes: 6,
            text: "Remove chicken. Sauté diced onion, peppers, and garlic until soft and a little caramelized.",
        },
        Step {
            minutes: 2,
            text: "Add rice; toast for 2 minutes until glossy.",
        },
        Step {
            minutes: 3,
            text: "Pour in stock, saffron, bay leaf. Bring to a simmer.",
        },
        Step {
            minutes: 35,
            text: "Return chicken. Cover, simmer on low heat until rice is tender and chicken cooked through.",
        },
        Step {
            minutes: 6,
            text: "Rest off heat 5 min. Fluff rice. Top with parsley and a squeeze of lime.",
        },
    ];
    const R6_INGREDIENTS: &[Ingredient] = &[
        Ingredient {
            name: "Dark chocolate 70%",
            qty: "200 g",
        },
        Ingredient {
            name: "Eggs",
            qty: "4",
        },
        Ingredient {
            name: "Heavy cream",
            qty: "200 ml",
        },
        Ingredient {
            name: "Caster sugar",
            qty: "40 g",
        },
        Ingredient {
            name: "Sea salt",
            qty: "1 pinch",
        },
    ];
    const R6_STEPS: &[Step] = &[
        Step {
            minutes: 5,
            text: "Melt chocolate gently over a bain-marie. Let cool to just warm.",
        },
        Step {
            minutes: 4,
            text: "Separate eggs. Whisk yolks into the chocolate one at a time.",
        },
        Step {
            minutes: 4,
            text: "Whip cream to soft peaks. Whip whites with sugar to glossy peaks.",
        },
        Step {
            minutes: 3,
            text: "Fold whipped cream into the chocolate. Then fold in the whites in three additions — keep it airy.",
        },
        Step {
            minutes: 4,
            text: "Spoon into glasses and chill at least 2 hours. Top with flaky salt and shaved chocolate to serve.",
        },
    ];

    vec![
        Recipe {
            id: "r1",
            title: "Arroz con Pollo",
            kind: MealType::Main,
            emoji: "🍛",
            cuisine: "Caribbean",
            time_minutes: 65,
            servings: 4,
            rating: 4.7,
            difficulty: "Medium",
            description: Some(
                "Saffron-scented rice braised with chicken, peppers, and a whisper of smoked paprika — a full one-pot dinner.",
            ),
            tags: &["Gluten-free", "One-pot"],
            ingredients: R1_INGREDIENTS,
            steps: R1_STEPS,
        },
        Recipe {
            id: "r2",
            title: "Carrot Cake",
            kind: MealType::Dessert,
            emoji: "🍰",
            cuisine: "American",
            time_minutes: 60,
            servings: 8,
            rating: 4.9,
            difficulty: "Easy",
            description: None,
            tags: &["Vegetarian"],
            ingredients: &[],
            steps: &[],
        },
        Recipe {
            id: "r3",
            title: "Tofu Vegetable Stir Fry",
            kind: MealType::Main,
            emoji: "🥘",
            cuisine: "Asian",
            time_minutes: 25,
            servings: 4,
            rating: 4.5,
            difficulty: "Easy",
            description: None,
            tags: &["Vegan", "Nut-free"],
            ingredients: &[],
            steps: &[],
        },
        Recipe {
            id: "r4",
            title: "Caesar Salad",
            kind: MealType::Entree,
            emoji: "🥗",
            cuisine: "Italian",
            time_minutes: 15,
            servings: 2,
            rating: 4.3,
            difficulty: "Easy",
            description: None,
            tags: &[],
            ingredients: &[],
            steps: &[],
        },
        Recipe {
            id: "r5",
            title: "Thai Red Curry",
            kind: MealType::Main,
            emoji: "🍲",
            cuisine: "Thai",
            time_minutes: 40,
            servings: 4,
            rating: 4.8,
            difficulty: "Medium",
            description: None,
            tags: &[],
            ingredients: &[],
            steps: &[],
        },
        Recipe {
            id: "r6",
            title: "Chocolate Mousse",
            kind: MealType::Dessert,
            emoji: "🍫",
            cuisine: "French",
            time_minutes: 20,
            servings: 4,
            rating: 4.6,
            difficulty: "Easy",
            description: Some(
                "Cloud-light bittersweet mousse — a handful of ingredients, all technique. Chill at least two hours before serving.",
            ),
            tags: &[],
            ingredients: R6_INGREDIENTS,
            steps: R6_STEPS,
        },
        Recipe {
            id: "r7",
            title: "Garlic Focaccia",
            kind: MealType::Side,
            emoji: "🥖",
            cuisine: "Italian",
            time_minutes: 180,
            servings: 6,
            rating: 4.4,
            difficulty: "Medium",
            description: None,
            tags: &[],
            ingredients: &[],
            steps: &[],
        },
        Recipe {
            id: "r8",
            title: "Miso Soup",
            kind: MealType::Entree,
            emoji: "🍜",
            cuisine: "Japanese",
            time_minutes: 15,
            servings: 4,
            rating: 4.2,
            difficulty: "Easy",
            description: None,
            tags: &[],
            ingredients: &[],
            steps: &[],
        },
        Recipe {
            id: "r9",
            title: "Pulled Pork Tacos",
            kind: MealType::Main,
            emoji: "🌮",
            cuisine: "Mexican",
            time_minutes: 240,
            servings: 6,
            rating: 4.9,
            difficulty: "Hard",
            description: None,
            tags: &[],
            ingredients: &[],
            steps: &[],
        },
        Recipe {
            id: "r10",
            title: "Caprese Plate",
            kind: MealType::Entree,
            emoji: "🧀",
            cuisine: "Italian",
            time_minutes: 10,
            servings: 4,
            rating: 4.4,
            difficulty: "Easy",
            description: None,
            tags: &[],
            ingredients: &[],
            steps: &[],
        },
        Recipe {
            id: "r11",
            title: "Roasted Potatoes",
            kind: MealType::Side,
            emoji: "🥔",
            cuisine: "American",
            time_minutes: 45,
            servings: 4,
            rating: 4.1,
            difficulty: "Easy",
            description: None,
            tags: &[],
            ingredients: &[],
            steps: &[],
        },
        Recipe {
            id: "r12",
            title: "Tiramisu",
            kind: MealType::Dessert,
            emoji: "☕",
            cuisine: "Italian",
            time_minutes: 240,
            servings: 6,
            rating: 4.8,
            difficulty: "Medium",
            description: None,
            tags: &[],
            ingredients: &[],
            steps: &[],
        },
    ]
}
