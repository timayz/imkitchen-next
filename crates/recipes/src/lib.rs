//! Recipes domain — DDD/CQRS/ES + saga for the recipes bounded context.
//!
//! Layout:
//!
//! - [`recipe`] — `Recipe` aggregate (events + commands). A recipe is the
//!   consistency boundary for one user-owned cooking artifact.
//! - [`import`] — `RecipeImport` aggregate + saga process manager that drives
//!   the multi-step import flow (source picked → preview ready → confirmed →
//!   recipes drafted). Stubs the external effects (HTTP fetch, OCR, parser)
//!   behind the [`import::parser::RecipeParser`] trait so they can be swapped
//!   later without touching the saga.
//! - [`projection`] — read models that feed the templates:
//!   - [`projection::recipes_view`] — recipe catalog + detail (one table,
//!     serves both `/recipes` index and `/recipes/{id}` detail).
//!   - [`projection::recipe_imports_view`] — current state of each import
//!     job, feeds the three import stages.
//! - [`subscriptions`] — wires projections + the import saga onto the event
//!   stream. Call [`subscriptions::start_all`] once at process boot.
//! - [`migrations`] — `sqlx_migrator` migrations for the read-side tables.
//!   The event store schema itself is owned by `evento` (the cli registers
//!   evento's migrator alongside ours).
//!
//! The aggregator type strings persisted in the event log are
//! `imkitchen-recipes/Recipe` and `imkitchen-recipes/RecipeImport`. Renaming
//! this crate would break replay; keep the package name stable.

pub mod import;
pub mod migrations;
pub mod projection;
pub mod recipe;
pub mod subscriptions;

pub use import::{ImportSource, ImportStage, ParsedCandidate};
pub use recipe::{IngredientFact, MealType, StepFact, Unit};
