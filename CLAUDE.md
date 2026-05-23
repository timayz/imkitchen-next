# imkitchen-next

Intelligent meal planning app. Generates personalized meal plans from recipes the user selects in the shared recipes catalog or contributes themselves, taking their meal preferences into account.

## Stack

Mobile-first PWA. Rust-based, primarily built on:
- `axum` 0.8.9 — HTTP server
- `askama` 0.16.0 — HTML templating
- `evento` 2.0.0 — event sourcing / CQRS
- TailwindCSS 4.3 — styling
- TwinSpark — htmx-like interactivity
- `clap` 4.6.1 — CLI argument parsing

## Auth

Cookie-based session auth. Extractors redirect to `/login` on failure.

- `web/server/src/auth.rs` — `User`, `Chef`, `Premium` extractors. Cookie `imkitchen_session` (HttpOnly, SameSite=Lax). `User` accepts any signed-in role; `Chef` and `Premium` each accept only their own role. Use them as handler arguments to gate routes.
- `admin/server/src/auth.rs` — `Admin` extractor. Cookie `imkitchen_admin_session` (separate from web).

Both servers expose `GET /login`, `POST /login`, `POST /logout`.
