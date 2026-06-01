//! Read-side projections.
//!
//! Each projection is just a function: (event, sql pool) → upsert. The
//! subscription module wires them onto the event stream.
//!
//! We use *one* `recipes_view` row per recipe (it serves both the index page
//! and the detail page). Two views would let the index and detail diverge in
//! shape; for SQLite + this domain the duplication isn't worth it.
//!
//! Idempotency: every upsert is `ON CONFLICT DO UPDATE` or guarded by
//! `is_subscriber_running`'s cursor — replaying the same event lands at the
//! same row state.

pub mod recipe_imports_view;
pub mod recipes_view;
