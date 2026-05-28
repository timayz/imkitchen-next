//! Import-recipe flow.
//!
//! GET  /recipes/import                              → upload screen
//! GET  /recipes/import?job=<id>&stage=preview|done  → an in-progress job
//! POST /recipes/import                              → start a new import
//!                                                     OR confirm picks
//!
//! Sharing one URL across stages keeps the chrome (top bar, breadcrumb, live
//! region) stable for AT users and makes link-sharing predictable.
//!
//! Pipeline (real, evento-backed):
//!
//! 1. POST with `next_stage=preview` and a `source` → dispatch
//!    `StartImport`. Aggregate id comes back. We redirect to
//!    `/recipes/import?job=<id>&stage=preview` (303 GET).
//! 2. GET `?job=<id>&stage=preview` → look up the import row. If the saga
//!    has already produced candidates, render the preview screen with them;
//!    otherwise show a "still parsing…" message (the saga polls every ~1s).
//! 3. POST with `next_stage=done` and the picked ids → dispatch
//!    `ConfirmImport`. We redirect to `/recipes/import?job=<id>&stage=done`.
//! 4. GET `?job=<id>&stage=done` → render the success stats.
//!
//! External effects (URL fetch, OCR, JSON parser) live behind the
//! `RecipeParser` trait in the recipes crate. Today only the seed parser is
//! wired; a future change can swap in real implementations without touching
//! this file.

use askama::Template;
use axum::{
    body::{Bytes, to_bytes},
    extract::{FromRequest, Multipart, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use imkitchen_recipes::{
    import::{ConfirmImport, ImportSource, StartImport, confirm_import, start_import},
    projection::recipe_imports_view::{self, ImportRow, ParsedCandidateView},
};
use serde::Deserialize;

use crate::{
    AppState,
    auth::{Role, User},
    recipes::{NavItem, owner_id},
};

/// See `recipes.rs` — same partial-detection logic, lifted because both
/// modules need it.
const TS_PARTIAL_ACCEPT: &str = "text/html+partial";

fn wants_partial(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains(TS_PARTIAL_ACCEPT))
}

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
    #[serde(default)]
    pub job: Option<String>,
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

/// One parsed-recipe candidate shown in the preview stage. Fields mirror
/// what `_import_preview.html` reads.
pub struct ParsedRecipe {
    pub id: String,
    pub title: String,
    pub emoji: String,
    pub type_slug: String,
    pub type_label: String,
    pub ingredient_count: u32,
    pub step_count: u32,
    pub warn: Option<String>,
    pub broken: bool,
    pub selected: bool,
}

impl ParsedRecipe {
    fn from_view(c: ParsedCandidateView) -> Self {
        let type_label = match c.meal_type.as_str() {
            "entree" => "Starter",
            "main" => "Main",
            "side" => "Side",
            "dessert" => "Dessert",
            _ => "Main",
        }
        .to_owned();
        Self {
            id: c.id,
            title: c.title,
            emoji: c.emoji,
            type_slug: c.meal_type,
            type_label,
            ingredient_count: c.ingredient_count,
            step_count: c.step_count,
            warn: c.warn,
            broken: c.broken,
            selected: c.selected,
        }
    }
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

    /// The URL-level stage (`"upload"` / `"preview"` / `"done"` / `"failed"`).
    /// Used to pick which sub-template to render. `"failed"` is implicit:
    /// the handler swaps in `"failed"` when the projection row terminated
    /// in `ImportFailed`.
    pub stage: &'static str,
    pub steps: Vec<StepIndicator>,
    pub job_id: String,

    /// Preview-stage poll mode. When `true`, render the `_import_waiting`
    /// shell (with a 300 ms `ts-trigger="load"`) instead of the preview
    /// content. The shell re-polls `/recipes/import?job=<id>&stage=preview`
    /// until the projection flips to `previewed` or `failed`.
    pub waiting: bool,

    // Upload-stage data
    pub source_options: Vec<SourceOption>,

    // Preview-stage data
    pub source_label: String,
    pub parsed: Vec<ParsedRecipe>,
    pub valid_count: usize,
    pub warning_count: usize,

    // Done-stage data
    pub imported_count: u32,
    pub stats: Vec<ImportStat>,

    /// Populated on the failed branch; otherwise empty.
    pub failure_reason: String,
}

/// Partial-only response used during preview polling. Single root section
/// with `ts-trigger="load delay:300ms"` so twinspark naturally re-arms.
#[derive(Template)]
#[template(path = "recipes/_import_waiting.html")]
pub struct ImportWaitingFragment {
    pub job_id: String,
}

/// Partial-only failure branch. No trigger, so polling stops on the client.
#[derive(Template)]
#[template(path = "recipes/_import_failed.html")]
pub struct ImportFailedFragment {
    pub failure_reason: String,
}

// ── Handlers ────────────────────────────────────────────────────────────

#[tracing::instrument(name = "recipes.import.page", skip(state, headers), fields(role = user.role.as_str(), stage = q.stage.slug(), job = ?q.job))]
pub async fn import_page(
    user: User,
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ImportQuery>,
) -> Response {
    let row = match q.job.as_deref() {
        Some(id) => match recipe_imports_view::find(&state.read_pool, id).await {
            Ok(r) => r,
            Err(err) => {
                tracing::error!(error = %err, "recipe_imports_view::find failed");
                return (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response();
            }
        },
        None => None,
    };

    // Owner scoping check: if the job exists, make sure it belongs to this
    // user. Otherwise fall back to a fresh upload screen.
    let owner = owner_id(&user);
    let row = row.filter(|r| r.owner_id == owner);

    // Preview-stage polling decisions. The projection's `stage` column is the
    // source of truth: it's `started` until the saga emits `ImportPreviewed`,
    // then flips to `previewed`. `failed` means the saga gave up; render the
    // terminal error and stop the loop. Anything else falls through to the
    // normal multi-stage page builder.
    if matches!(q.stage, Stage::Preview)
        && let Some(ref row) = row
    {
        match row.stage.as_str() {
            "started" => {
                // Still parsing — render the polling shell (partial or full).
                if wants_partial(&headers) {
                    return render(ImportWaitingFragment {
                        job_id: row.id.clone(),
                    });
                }
                return render(build_page(&user, Stage::Preview, Some(row.clone()), true));
            }
            "failed" => {
                if wants_partial(&headers) {
                    return render(ImportFailedFragment {
                        failure_reason: row.failure_reason.clone(),
                    });
                }
                return render(build_page(&user, Stage::Preview, Some(row.clone()), false));
            }
            "previewed" if wants_partial(&headers) => {
                // Ready. Since this is a poll (partial accept), kick the
                // browser to the clean URL so it falls out of partial-response
                // mode and renders the full preview page on the next request.
                let mut resp = (StatusCode::OK, "").into_response();
                let url = format!("/recipes/import?job={}&stage=preview", row.id);
                if let Ok(value) = HeaderValue::from_str(&url) {
                    resp.headers_mut().insert("ts-location", value);
                }
                return resp;
            }
            "previewed" => {
                // Browser GET — fall through to render the preview page.
            }
            _ => {}
        }
    }

    render(build_page(&user, q.stage, row, false))
}

#[tracing::instrument(name = "recipes.import.submit", skip(state, request), fields(role = user.role.as_str()))]
pub async fn import_submit(
    user: User,
    State(state): State<AppState>,
    request: Request,
) -> Response {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();

    if content_type.starts_with("multipart/form-data") {
        return submit_multipart(&user, &state, request).await;
    }

    let body = match to_bytes(request.into_body(), state.config.body_limit_bytes).await {
        Ok(b) => b,
        Err(err) => {
            tracing::warn!(error = %err, "import body read failed");
            return (StatusCode::BAD_REQUEST, "couldn't read body").into_response();
        }
    };
    submit_urlencoded(&user, &state, &body).await
}

async fn submit_urlencoded(user: &User, state: &AppState, body: &Bytes) -> Response {
    let form = parse_submit(body);

    match form.next_stage {
        Some(Stage::Preview) => {
            let Some(source) = form
                .source
                .as_deref()
                .and_then(ImportSource::parse)
            else {
                return (StatusCode::BAD_REQUEST, "unknown source").into_response();
            };
            let label = form
                .source_label
                .unwrap_or_else(|| default_source_label(source));

            let cmd = StartImport {
                owner_id: owner_id(user),
                source,
                source_label: label,
            };
            match start_import(cmd, &state.evento).await {
                Ok(id) => Redirect::to(&format!("/recipes/import?job={id}&stage=preview"))
                    .into_response(),
                Err(err) => {
                    tracing::error!(error = %err, "start_import failed");
                    (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
                }
            }
        }
        Some(Stage::Done) => {
            let Some(job_id) = form.job_id else {
                return (StatusCode::BAD_REQUEST, "missing job id").into_response();
            };
            if form.picked.is_empty() {
                return (StatusCode::BAD_REQUEST, "no recipes picked").into_response();
            }
            match confirm_import(
                ConfirmImport {
                    import_id: job_id.clone(),
                    picked_ids: form.picked,
                },
                &state.evento,
            )
            .await
            {
                Ok(()) => Redirect::to(&format!("/recipes/import?job={job_id}&stage=done"))
                    .into_response(),
                Err(err) => {
                    tracing::error!(error = %err, "confirm_import failed");
                    (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
                }
            }
        }
        Some(Stage::Upload) | None => Redirect::to("/recipes/import").into_response(),
    }
}

/// Multipart upload path. Only one shape is supported today: a `<form>` from
/// `_import_upload.html` carrying a single `upload` file field plus the
/// hidden `source=file` / `next_stage=preview` markers. The actual file is
/// passed to the configured `RecipeParser::parse_file`; the resulting
/// candidates are pushed into the saga via the same `StartImport` →
/// `RecordPreview` path that the urlencoded flow uses (no need for the
/// saga's own `parse()` step — we already have the candidates).
async fn submit_multipart(user: &User, state: &AppState, request: Request) -> Response {
    let multipart_extractor = Multipart::from_request(request, &()).await;
    let mut multipart = match multipart_extractor {
        Ok(m) => m,
        Err(err) => {
            tracing::warn!(error = %err, "multipart parse failed");
            return import_error_response(user, "We couldn't read that upload.");
        }
    };

    let mut file_name = String::new();
    let mut content_type = String::new();
    let mut file_bytes: Vec<u8> = Vec::new();

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(err) => {
                tracing::warn!(error = %err, "multipart field read failed");
                return import_error_response(user, "That upload was malformed.");
            }
        };

        if field.name() != Some("upload") {
            continue;
        }
        file_name = field.file_name().unwrap_or("upload.json").to_owned();
        content_type = field.content_type().unwrap_or("").to_owned();
        match field.bytes().await {
            Ok(b) => file_bytes = b.to_vec(),
            Err(err) => {
                tracing::warn!(error = %err, "multipart body read failed");
                return import_error_response(user, "Couldn't read that file.");
            }
        }
        // First `upload` field wins. The upload form only has one anyway.
        break;
    }

    if file_bytes.is_empty() {
        return import_error_response(user, "Pick a file before uploading.");
    }
    if !is_acceptable_json(&file_name, &content_type) {
        return import_error_response(
            user,
            "Only .json recipe files are supported right now.",
        );
    }

    let candidates = match state
        .recipe_parser
        .parse_file(&file_name, &content_type, &file_bytes)
        .await
    {
        Ok(c) if !c.is_empty() => c,
        Ok(_) => {
            return import_error_response(user, "No recipes found in the file.");
        }
        Err(err) => {
            tracing::warn!(error = %err, "parse_file failed");
            return import_error_response(user, &format!("Couldn't parse: {err}"));
        }
    };

    // Open the import aggregate with the real file name as label; immediately
    // record the preview candidates so the saga doesn't need to re-run its
    // own `parse()` step. The saga's `on_started` handler also calls
    // `parse()`, but since the projection's candidate list converges via the
    // last write the user sees the same outcome either way.
    let owner_id_str = owner_id(user);
    let import_id = match start_import(
        StartImport {
            owner_id: owner_id_str,
            source: ImportSource::File,
            source_label: file_name,
        },
        &state.evento,
    )
    .await
    {
        Ok(id) => id,
        Err(err) => {
            tracing::error!(error = %err, "start_import failed");
            return import_error_response(user, "Couldn't start the import.");
        }
    };

    // Pre-seed the preview with the file's candidates by writing
    // `ImportPreviewed` directly. We do this *after* `ImportStarted` so the
    // projection sees both events in order and the saga's own (redundant)
    // `parse()` lands as a no-op upsert. We use `Projection` to read the
    // current version because saga commits are concurrent.
    if let Err(err) =
        seed_preview(&state.evento, &import_id, candidates.clone()).await
    {
        tracing::warn!(error = %err, import_id = %import_id, "preview seed failed");
        // Non-fatal — the saga will compute candidates itself.
    }

    Redirect::to(&format!("/recipes/import?job={import_id}&stage=preview")).into_response()
}

/// Accept the upload only when the extension or content-type signals JSON.
fn is_acceptable_json(file_name: &str, content_type: &str) -> bool {
    let ext_ok = file_name
        .rsplit_once('.')
        .map(|(_, ext)| ext.eq_ignore_ascii_case("json"))
        .unwrap_or(false);
    let ct = content_type.split(';').next().unwrap_or("").trim();
    ext_ok || matches!(ct, "application/json" | "text/json")
}

/// Re-render the upload screen with an inline error banner. The upload
/// template doesn't currently render an error region, so the message goes
/// into the `source_label` slot to surface to the user.
fn import_error_response(user: &User, message: &str) -> Response {
    let mut page = build_page(user, Stage::Upload, None, false);
    page.failure_reason = message.to_owned();
    // Show the failed branch even though we're at the upload stage — it has
    // a "Start over" CTA back to /recipes/import which is what we want.
    page.stage = "failed";
    let mut resp = render(page);
    *resp.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
    resp
}

/// Write an `ImportPreviewed` event so the projection immediately shows the
/// file's candidates without waiting for the saga's own `parse()` cycle.
async fn seed_preview(
    executor: &evento::Sqlite,
    import_id: &str,
    candidates: Vec<imkitchen_recipes::import::ParsedCandidate>,
) -> anyhow::Result<()> {
    use imkitchen_recipes::import::ImportPreviewed;
    // Read latest version from the aggregate to set `original_version` for
    // optimistic concurrency. The saga's own parse step may race us; whichever
    // one commits second hits the version check and the projection ends up
    // with the same candidates either way (we and the saga both compute from
    // the same file).
    let read = evento::Executor::read(
        executor,
        Some(vec![evento::ReadAggregator::id(
            <imkitchen_recipes::import::ImportStarted as evento::Aggregator>::aggregator_type(),
            import_id,
        )]),
        None,
        evento::cursor::Args::backward(1, None),
    )
    .await?;
    let original_version = read
        .edges
        .into_iter()
        .next()
        .map(|e| e.node.version)
        .unwrap_or(0);

    evento::aggregator(import_id)
        .original_version(original_version)
        .event(&ImportPreviewed { candidates })
        .commit(executor)
        .await
        .map_err(|e| anyhow::anyhow!("seed_preview commit failed: {e}"))?;
    Ok(())
}


// ── Helpers ─────────────────────────────────────────────────────────────

fn build_page(
    user: &User,
    stage: Stage,
    row: Option<ImportRow>,
    waiting: bool,
) -> ImportPage {
    // Preview stage needs candidate data; done stage needs the recipe-id list.
    let (parsed, source_label, imported_count, picked_count) = match (&row, stage) {
        (Some(row), Stage::Preview) => {
            let candidates = row.candidates();
            let parsed: Vec<ParsedRecipe> =
                candidates.into_iter().map(ParsedRecipe::from_view).collect();
            (parsed, row.source_label.clone(), 0, 0)
        }
        (Some(row), Stage::Done) => {
            let imported = row.recipe_ids().len() as u32;
            let picked = row.picked_ids().len();
            (Vec::new(), row.source_label.clone(), imported, picked)
        }
        _ => (Vec::new(), String::new(), 0, 0),
    };

    let valid_count = parsed.iter().filter(|r| !r.broken).count();
    let warning_count = parsed.iter().filter(|r| r.warn.is_some()).count();
    let job_id = row.as_ref().map(|r| r.id.clone()).unwrap_or_default();

    // Terminal-failure detection. Any non-`upload` URL stage where the row is
    // marked failed renders the failure branch. (For the upload stage there
    // is no job yet, so this is a no-op.)
    let failure_reason = row
        .as_ref()
        .filter(|r| r.stage == "failed")
        .map(|r| r.failure_reason.clone())
        .unwrap_or_default();
    let effective_stage: &'static str =
        if !failure_reason.is_empty() && !matches!(stage, Stage::Upload) {
            "failed"
        } else {
            stage.slug()
        };

    let stats = if matches!(stage, Stage::Done) {
        let skipped = picked_count.saturating_sub(imported_count as usize) as u32;
        vec![
            ImportStat {
                label: "Imported",
                count: imported_count,
            },
            ImportStat {
                label: "Skipped",
                count: skipped,
            },
            ImportStat {
                label: "Duplicate",
                count: 0,
            },
        ]
    } else {
        Vec::new()
    };

    ImportPage {
        nav_items: crate::recipes::nav_items(),
        user_initial: crate::recipes::user_initial(user),
        user_name: "Jenny Rosen",
        user_email: "jenny@imkitchen.app",
        user_premium: matches!(user.role, Role::Premium),

        stage: effective_stage,
        steps: step_indicators(stage),
        job_id,
        waiting,

        source_options: source_options(),

        source_label,
        parsed,
        valid_count,
        warning_count,

        imported_count,
        stats,
        failure_reason,
    }
}

struct SubmitForm {
    next_stage: Option<Stage>,
    source: Option<String>,
    source_label: Option<String>,
    job_id: Option<String>,
    picked: Vec<String>,
}

fn parse_submit(body: &[u8]) -> SubmitForm {
    let mut next_stage = None;
    let mut source = None;
    let mut source_label = None;
    let mut job_id = None;
    let mut picked = Vec::new();

    for (key, value) in form_urlencoded::parse(body) {
        match key.as_ref() {
            "next_stage" => {
                next_stage = match value.as_ref() {
                    "upload" => Some(Stage::Upload),
                    "preview" => Some(Stage::Preview),
                    "done" => Some(Stage::Done),
                    _ => None,
                };
            }
            "source" => source = Some(value.into_owned()),
            "source_label" => source_label = Some(value.into_owned()),
            "job" => job_id = Some(value.into_owned()),
            "pick" => picked.push(value.into_owned()),
            _ => {}
        }
    }

    SubmitForm {
        next_stage,
        source,
        source_label,
        job_id,
        picked,
    }
}

fn default_source_label(source: ImportSource) -> String {
    // The upload screen doesn't ask the user for a file name yet — until
    // multipart parsing lands, pick something readable per source kind so
    // the preview header isn't blank.
    match source {
        ImportSource::Url => "Pasted URL".into(),
        ImportSource::Photo => "Captured photo".into(),
        ImportSource::Text => "Pasted text".into(),
        ImportSource::Share => "Share link".into(),
        ImportSource::File => "Uploaded file".into(),
    }
}

fn render<T: Template>(tmpl: T) -> Response {
    match tmpl.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
