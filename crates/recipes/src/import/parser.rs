//! Stub seam for the external import parsers (URL / OCR / text / file).
//!
//! Today none of the four are real — see the TODOs in `web/server` for the
//! actual integrations. The saga only ever talks to a `RecipeParser` trait
//! object, so swapping in a real implementation later is one wiring change
//! at startup, not a sweep through the saga.
//!
//! ### Design notes
//!
//! - The trait returns `anyhow::Result` so an implementation can surface a
//!   user-readable failure reason. The saga maps `Err` to `ImportFailed` with
//!   that reason as the message.
//! - The default impl ([`SeedParser`]) returns a deterministic candidate list
//!   regardless of source. It's not "random sample data": it's the same shape
//!   the original mockup shipped with, so screenshots still match.
//! - The parsed `Recipe` payload here is *only* what the preview UI needs
//!   (title, counts, warnings). The full ingredient/step content for the
//!   confirmed picks is materialized by the saga at draft time from a
//!   hard-coded recipe template; in production this becomes a follow-up
//!   "fetch full body" call on the parser.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::recipe::{IngredientFact, MealType, StepFact, Unit};

use super::ImportSource;

/// One row in the preview list. The fields map 1-1 onto
/// `recipes/_import_preview.html`'s `ParsedRecipe` struct so the projection
/// can pass it straight through.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, bitcode::Encode, bitcode::Decode)]
pub struct ParsedCandidate {
    pub id: String,
    pub title: String,
    pub emoji: String,
    /// One of the meal-type slugs (`"main"`, `"side"`, …). Free-form here
    /// because the type might be unknown for some parsers; the projection
    /// falls back to "main" when it doesn't recognize the slug.
    pub meal_type: String,
    pub ingredient_count: u32,
    pub step_count: u32,
    /// Warning that surfaces under the candidate ("Similar to existing …").
    pub warn: Option<String>,
    /// `true` when the candidate cannot be imported (e.g. missing title) —
    /// the checkbox is rendered disabled and the saga skips the row.
    pub broken: bool,
    /// Initial checkbox state on the preview screen.
    pub selected: bool,
}

/// External-effect seam for the import flow.
///
/// Implementations may hit the network (URL fetch), call OCR (`Photo`), or
/// parse uploaded files (`File`). They MUST NOT mutate the event store —
/// the saga is the only thing that turns parser output into events.
#[async_trait::async_trait]
pub trait RecipeParser: Send + Sync {
    /// Parse a source into the preview candidate list.
    async fn parse(&self, source: ImportSource, label: &str) -> Result<Vec<ParsedCandidate>>;

    /// Parse an uploaded file into the preview candidate list.
    ///
    /// Called by the multipart upload path in the web layer; the `file_name`
    /// is used as the source label so the preview screen shows the real file
    /// rather than a generic placeholder.
    async fn parse_file(
        &self,
        file_name: &str,
        content_type: &str,
        bytes: &[u8],
    ) -> Result<Vec<ParsedCandidate>>;

    /// Materialize the full draftable form for one candidate. Called once per
    /// confirmed pick. The default seed parser returns a fixed template; a
    /// real implementation would re-fetch / re-parse to get the full body.
    async fn materialize(&self, candidate: &ParsedCandidate) -> Result<DraftMaterial>;
}

/// All the data the saga needs to draft a `Recipe` from a confirmed candidate.
#[derive(Debug, Clone)]
pub struct DraftMaterial {
    pub title: String,
    pub meal_type: MealType,
    pub cuisine: String,
    pub emoji: String,
    pub prep_minutes: u32,
    pub cook_minutes: u32,
    pub servings: u32,
    pub difficulty: String,
    pub description: String,
    pub tags: Vec<String>,
    pub ingredients: Vec<IngredientFact>,
    pub steps: Vec<StepFact>,
}

/// Default no-network parser. Hands back the same 8-recipe preview list the
/// mockup ships with, so the import flow is end-to-end testable without any
/// external services running.
pub struct SeedParser;

#[async_trait::async_trait]
impl RecipeParser for SeedParser {
    async fn parse(&self, _source: ImportSource, _label: &str) -> Result<Vec<ParsedCandidate>> {
        Ok(seed_candidates())
    }

    /// Real JSON parsing when bytes are present; otherwise the seed list.
    ///
    /// Empty-bytes fallback exists so that callers exercising the trait in
    /// stub mode (e.g. saga unit tests) still get a non-empty preview list
    /// without needing to construct a synthetic JSON payload.
    async fn parse_file(
        &self,
        _file_name: &str,
        _content_type: &str,
        bytes: &[u8],
    ) -> Result<Vec<ParsedCandidate>> {
        if bytes.is_empty() {
            return Ok(seed_candidates());
        }
        parse_recipes_json(bytes)
    }

    async fn materialize(&self, candidate: &ParsedCandidate) -> Result<DraftMaterial> {
        Ok(materialize_seed(candidate))
    }
}

// ── Real JSON parsing ──────────────────────────────────────────────────
//
// Strict shape:
//
// ```json
// { "title": "…", "ingredients": [{"name": "…", "quantity": 250, "unit": "g"}, …], "steps": [{"text": "…", "wait_minutes": 5}, …], "meal_type": "main", "emoji": "🍲", "cuisine": "…", "difficulty": "…", "description": "…", "tags": ["…"], "servings": 4, "prep_minutes": 10, "cook_minutes": 20 }
// ```
//
// Or an array of the same. Required fields: `title` and `ingredients`. Every
// other field has a sensible default. Anything else fails with a polite
// error the upload screen surfaces inline.

/// Top-level JSON shape: either one recipe or a list of them.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RecipesPayload {
    One(RecipeImport),
    Many(Vec<RecipeImport>),
}

/// One recipe as it appears in an uploaded JSON file.
///
/// We only deserialize what the preview screen needs right now (`title`,
/// `meal_type`, `emoji`, and counts of ingredients / steps). The full body
/// fields (`quantity`, `unit`, `text`, `wait_minutes`, `cuisine`, `tags`, etc.) get parsed by
/// `serde` too — they just live on `serde_json::Value` lists so we don't
/// have to enumerate every optional and then leave them unused. When a
/// future change materializes a file-imported draft, it'll read those
/// values straight off the deserialized JSON for the picked candidate.
///
/// Required fields: `title` (string) and `ingredients` (array). Anything
/// else is permissive.
#[derive(Debug, Deserialize)]
struct RecipeImport {
    title: String,
    #[serde(default)]
    meal_type: Option<String>,
    #[serde(default)]
    emoji: Option<String>,
    ingredients: Vec<serde_json::Value>,
    #[serde(default)]
    steps: Vec<serde_json::Value>,
}

pub(crate) fn parse_recipes_json(bytes: &[u8]) -> Result<Vec<ParsedCandidate>> {
    let payload: RecipesPayload = serde_json::from_slice(bytes)
        .map_err(|e| anyhow::anyhow!("That doesn't look like recipe JSON: {e}"))?;

    let recipes = match payload {
        RecipesPayload::One(r) => vec![r],
        RecipesPayload::Many(rs) => rs,
    };

    if recipes.is_empty() {
        bail!("No recipes found in the file.");
    }

    let candidates: Vec<ParsedCandidate> = recipes
        .into_iter()
        .enumerate()
        .map(|(idx, r)| {
            let meal_type = r
                .meal_type
                .as_deref()
                .and_then(MealType::parse)
                .unwrap_or(MealType::Main);
            let title_present = !r.title.trim().is_empty();
            let ingredient_count = r.ingredients.len() as u32;
            let step_count = r.steps.len() as u32;
            let broken = !title_present || r.ingredients.is_empty();
            let warn = if broken {
                Some("Missing title or ingredients — can't import".to_owned())
            } else {
                None
            };
            ParsedCandidate {
                id: format!("f{}", idx + 1),
                title: if title_present {
                    r.title
                } else {
                    "Untitled recipe".into()
                },
                emoji: r
                    .emoji
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| meal_type.default_emoji().to_owned()),
                meal_type: meal_type.slug().to_owned(),
                ingredient_count,
                step_count,
                warn,
                broken,
                selected: !broken,
            }
        })
        .collect();

    Ok(candidates)
}

// ── Seed data ───────────────────────────────────────────────────────────
//
// Mirrors `recipes_import.rs::stub_parsed()` so the screen still looks like
// the mockup. The 8th row (`i8`) is intentionally broken — the import flow
// is supposed to demonstrate that warning state.

fn seed_candidates() -> Vec<ParsedCandidate> {
    vec![
        ParsedCandidate {
            id: "i1".into(),
            title: "Grandma's Pot Roast".into(),
            emoji: "🥩".into(),
            meal_type: "main".into(),
            ingredient_count: 9,
            step_count: 6,
            warn: None,
            broken: false,
            selected: true,
        },
        ParsedCandidate {
            id: "i2".into(),
            title: "Green Bean Casserole".into(),
            emoji: "🥗".into(),
            meal_type: "side".into(),
            ingredient_count: 7,
            step_count: 5,
            warn: None,
            broken: false,
            selected: true,
        },
        ParsedCandidate {
            id: "i3".into(),
            title: "Buttermilk Biscuits".into(),
            emoji: "🥐".into(),
            meal_type: "side".into(),
            ingredient_count: 6,
            step_count: 4,
            warn: None,
            broken: false,
            selected: true,
        },
        ParsedCandidate {
            id: "i4".into(),
            title: "Apple Pie".into(),
            emoji: "🥧".into(),
            meal_type: "dessert".into(),
            ingredient_count: 10,
            step_count: 8,
            warn: None,
            broken: false,
            selected: true,
        },
        ParsedCandidate {
            id: "i5".into(),
            title: "Pecan Bars".into(),
            emoji: "🍪".into(),
            meal_type: "dessert".into(),
            ingredient_count: 8,
            step_count: 5,
            warn: Some("Similar to existing \"Pecan Squares\"".into()),
            broken: false,
            selected: false,
        },
        ParsedCandidate {
            id: "i6".into(),
            title: "Deviled Eggs".into(),
            emoji: "🥚".into(),
            meal_type: "entree".into(),
            ingredient_count: 6,
            step_count: 3,
            warn: None,
            broken: false,
            selected: true,
        },
        ParsedCandidate {
            id: "i7".into(),
            title: "Sweet Potato Mash".into(),
            emoji: "🍠".into(),
            meal_type: "side".into(),
            ingredient_count: 5,
            step_count: 4,
            warn: None,
            broken: false,
            selected: true,
        },
        ParsedCandidate {
            id: "i8".into(),
            title: "Untitled recipe".into(),
            emoji: "🍽️".into(),
            meal_type: "main".into(),
            ingredient_count: 0,
            step_count: 0,
            warn: Some("Missing title and steps — can't import".into()),
            broken: true,
            selected: false,
        },
    ]
}

/// Produce a usable draft template for one candidate. Real parsers replace
/// this with the actual scraped body.
fn materialize_seed(candidate: &ParsedCandidate) -> DraftMaterial {
    let meal_type = MealType::parse(&candidate.meal_type).unwrap_or(MealType::Main);

    let ingredients: Vec<IngredientFact> = (1..=candidate.ingredient_count.max(1))
        .map(|i| IngredientFact {
            name: format!("Ingredient {i}"),
            quantity: None,
            unit: Unit::None,
        })
        .collect();
    let steps: Vec<StepFact> = (1..=candidate.step_count.max(1))
        .map(|i| StepFact {
            wait_minutes: 0,
            text: format!("Step {i} — refine after import."),
        })
        .collect();

    DraftMaterial {
        title: candidate.title.clone(),
        meal_type,
        cuisine: "Imported".into(),
        emoji: candidate.emoji.clone(),
        prep_minutes: 10,
        cook_minutes: 20,
        servings: 4,
        difficulty: "Easy".into(),
        description: String::new(),
        tags: Vec::new(),
        ingredients,
        steps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_json_single_recipe() {
        let bytes = br#"
        {
            "title": "Tomato Soup",
            "meal_type": "main",
            "ingredients": [{"name": "Tomatoes", "quantity": 1, "unit": "kg"}],
            "steps": [{"text": "Simmer.", "wait_minutes": 30}]
        }"#;
        let out = parse_recipes_json(bytes).expect("valid single");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, "Tomato Soup");
        assert_eq!(out[0].meal_type, "main");
        assert!(!out[0].broken);
        assert!(out[0].selected);
        assert_eq!(out[0].ingredient_count, 1);
        assert_eq!(out[0].step_count, 1);
    }

    #[test]
    fn parse_json_array_of_recipes() {
        let bytes = br#"[
            { "title": "A", "ingredients": [{"name": "x"}] },
            { "title": "B", "ingredients": [{"name": "y"}] }
        ]"#;
        let out = parse_recipes_json(bytes).expect("valid array");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "A");
        assert_eq!(out[1].title, "B");
    }

    #[test]
    fn parse_json_rejects_garbage() {
        let err = parse_recipes_json(b"not json").unwrap_err();
        assert!(err.to_string().contains("recipe JSON"));
    }

    #[test]
    fn parse_json_rejects_empty_array() {
        let err = parse_recipes_json(b"[]").unwrap_err();
        assert!(err.to_string().contains("No recipes"));
    }

    #[test]
    fn parse_json_marks_missing_title_broken() {
        let bytes = br#"{ "title": "", "ingredients": [{"name":"x"}] }"#;
        let out = parse_recipes_json(bytes).expect("valid shape");
        assert_eq!(out.len(), 1);
        assert!(out[0].broken);
        assert!(!out[0].selected);
        assert_eq!(out[0].title, "Untitled recipe");
    }

    #[test]
    fn parse_json_marks_missing_ingredients_broken() {
        let bytes = br#"{ "title": "X", "ingredients": [] }"#;
        let out = parse_recipes_json(bytes).expect("valid shape");
        assert_eq!(out.len(), 1);
        assert!(out[0].broken);
        assert!(out[0].warn.is_some());
    }

    #[test]
    fn parse_json_defaults_meal_type_main() {
        let bytes = br#"{ "title": "X", "ingredients": [{"name":"x"}] }"#;
        let out = parse_recipes_json(bytes).unwrap();
        assert_eq!(out[0].meal_type, "main");
        // Default emoji from meal type when none provided.
        assert_eq!(out[0].emoji, MealType::Main.default_emoji());
    }
}
