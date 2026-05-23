---
name: tracing-logging
description: Enforce using the `tracing` crate for all logging and diagnostics in Rust code. Use whenever writing, editing, or reviewing Rust code that emits log output, debug output, error reports, or instrumentation. Bans `println!`, `eprintln!`, `print!`, `eprint!`, `dbg!`, and the `log` crate (`log::info!`, `log::error!`, etc.) for diagnostic purposes — they must be replaced with the equivalent `tracing` macro. Also covers span usage, `#[tracing::instrument]`, structured fields, and error logging conventions.
disable-model-invocation: true
---

# Tracing — Mandatory Logging Convention

All diagnostic output in this codebase MUST go through the [`tracing`](https://docs.rs/tracing) crate. Do not use `println!`, `eprintln!`, `dbg!`, or the `log` crate facade for logging.

## Allowed vs forbidden

| Purpose                          | Use                                  | Do NOT use                                |
| -------------------------------- | ------------------------------------ | ----------------------------------------- |
| Informational message            | `tracing::info!`                     | `println!`, `log::info!`                  |
| Debug-level detail               | `tracing::debug!`                    | `println!`, `dbg!`, `log::debug!`         |
| Very fine-grained trace          | `tracing::trace!`                    | `println!`, `log::trace!`                 |
| Recoverable problem              | `tracing::warn!`                     | `eprintln!`, `log::warn!`                 |
| Error / failure                  | `tracing::error!`                    | `eprintln!`, `log::error!`, `dbg!`        |
| Function/span entry instrumentation | `#[tracing::instrument]`           | manual `println!` at function start/end   |

**Exception:** CLI tools whose primary output is user-facing text (e.g., a `--help` printer, a final result printed to stdout) may still use `println!` for **program output**. Logging/diagnostics — anything a user wouldn't consider the program's "answer" — must use `tracing`.

## Import convention

Prefer the macro path inline so the call site is unambiguous:

```rust
tracing::info!(user_id = %id, "user logged in");
tracing::error!(error = ?err, "failed to load config");
```

A `use tracing::{info, warn, error, debug, trace, instrument};` at the top of a module is also fine if the file uses several macros.

## Structured fields, not string formatting

Use `tracing`'s key-value field syntax so events stay machine-parseable. Prefer fields over `format!`-style string interpolation.

```rust
// GOOD — structured
tracing::info!(order_id = %order.id, total_cents = order.total_cents, "order placed");

// BAD — opaque string
tracing::info!("order {} placed for {} cents", order.id, order.total_cents);
```

Field value sigils:
- `%value` → `Display`
- `?value` → `Debug`
- bare `value` → requires `Value` impl (numeric, bool, &str, etc.)

## Errors

When logging an error, attach it as a field — do not just stringify it into the message:

```rust
match do_thing().await {
    Ok(v) => v,
    Err(err) => {
        tracing::error!(error = ?err, "do_thing failed");
        return Err(err.into());
    }
}
```

For `anyhow::Error`, `?err` gives the full chain; `%err` gives only the top-level message. Prefer `?err` for errors so the cause chain is captured.

## Spans and `#[instrument]`

For functions whose execution boundaries matter (async handlers, commands, subscription handlers, request entry points), annotate with `#[tracing::instrument]`:

```rust
#[tracing::instrument(skip(db), fields(user_id = %req.user_id))]
async fn place_order(db: &Db, req: PlaceOrder) -> anyhow::Result<OrderId> {
    tracing::debug!("validating order");
    // ...
    Ok(order_id)
}
```

Guidelines:
- `skip(...)` large or non-`Debug` arguments (db pools, connections, large payloads).
- Add identifying fields with `fields(...)` so spans are filterable.
- Default level is `INFO`. Use `level = "debug"` for chatty internals.
- Do not annotate trivial getters/pure functions — instrumentation has a cost.

## Levels — when to use which

- `error!` — operation failed; user-visible or requires attention.
- `warn!`  — something unexpected but recovered (retry succeeded, fallback used, deprecated path hit).
- `info!`  — significant lifecycle events (server started, job completed, user action).
- `debug!` — internal state useful when diagnosing a problem.
- `trace!` — extremely verbose; per-iteration or per-message detail.

If you're tempted to write `dbg!(x)`, write `tracing::debug!(?x, "checkpoint")` instead.

## Setup (already present in this repo)

The crate is already wired up — `tracing` and `tracing-subscriber` are in `Cargo.toml`. A subscriber must be initialized once at program start:

```rust
tracing_subscriber::fmt()
    .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
    .init();
```

If you add tracing to a binary that has no subscriber, initialize one in `main` before any logging happens.

## Reviewing existing code

When editing a file, opportunistically convert any `println!`/`eprintln!`/`dbg!` calls used for diagnostics in the lines you touch. Do not do a sweeping repo-wide rewrite as part of an unrelated change — fix what you're already in.
