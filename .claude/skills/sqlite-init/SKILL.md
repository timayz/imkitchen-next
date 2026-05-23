---
name: sqlite-init
description: Reference for initializing SQLite connection pools with `sqlx` in Rust. Use whenever writing, editing, or reviewing code that creates `SqlitePool`s, configures SQLite pragmas, sets `journal_mode` / `synchronous` / `busy_timeout` / `foreign_keys`, or splits read vs write pools. Covers the read-only pool (sized to CPU cores, no WAL), the single-connection write pool (WAL + NORMAL synchronous, serializes writes to avoid `SQLITE_BUSY`), and the standard single-pool setup for CLI/test contexts. Also flags common anti-patterns: setting `journal_mode = WAL` on a read-only pool, omitting `busy_timeout`, missing `BEGIN IMMEDIATE` for write transactions, and applying pragmas in app code instead of `connect_with` (which loses them on replacement connections).
disable-model-invocation: true
---

# SQLite — Connection Pool Initialization

This codebase uses [`sqlx`](https://docs.rs/sqlx) with SQLite. All pools MUST be created via the helpers in this file's `base_options` pattern so per-connection pragmas survive idle-replacement of connections.

## The three pools

| Pool                 | Purpose                                          | Max connections   | `journal_mode` | `synchronous` |
| -------------------- | ------------------------------------------------ | ----------------- | -------------- | ------------- |
| `create_read_pool`   | Concurrent reads from web handlers / queries     | CPU cores         | (not set)      | (not set)     |
| `create_write_pool`  | Serialized writes — all `BEGIN IMMEDIATE` txns   | **1**             | `WAL`          | `NORMAL`      |
| `create_pool`        | CLI tools (migrate, import, tests) — single pool | caller-specified  | `WAL`          | `NORMAL`      |

Pick the pair (`read_pool` + `write_pool`) for the long-running server. Use the single `create_pool` for short-lived CLI commands and tests.

## Reference implementation

```rust
use anyhow::Result;
use sqlx::ConnectOptions;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use std::str::FromStr;
use std::time::Duration;
use tracing::log::LevelFilter;

fn base_options(database_url: &str) -> Result<SqliteConnectOptions> {
    Ok(SqliteConnectOptions::from_str(database_url)?
        .busy_timeout(Duration::from_millis(5000))
        .foreign_keys(true)
        .pragma("cache_size", "-20000")
        .pragma("temp_store", "memory")
        .log_statements(LevelFilter::Debug))
}

pub async fn create_read_pool(database_url: &str, max_connections: u32) -> Result<SqlitePool> {
    let options = base_options(database_url)?.read_only(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections)
        .connect_with(options)
        .await?;

    tracing::info!(
        "Created read-only pool with {} max connections",
        max_connections
    );
    Ok(pool)
}

pub async fn create_write_pool(database_url: &str) -> Result<SqlitePool> {
    let options = base_options(database_url)?
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;

    tracing::info!("Created read-write pool with 1 max connection");
    Ok(pool)
}

pub async fn create_pool(database_url: &str, max_connections: u32) -> Result<SqlitePool> {
    let options = base_options(database_url)?
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal);

    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections)
        .connect_with(options)
        .await?;

    tracing::info!("Created pool with {} max connections", max_connections);
    Ok(pool)
}
```

## Why this shape

### `base_options` returns a builder, not a connection

`SqliteConnectOptions` is **per-connection** state. `sqlx` re-applies it every time the pool opens a new connection — including replacement connections created after `idle_timeout` evicts an old one. If you set pragmas via `pool.execute("PRAGMA ...")` after the pool is built, those pragmas are lost on the *next* connection sqlx opens. Always set pragmas inside `base_options` so they survive connection replacement.

### Read pool: no `journal_mode`, no `synchronous`

Both are **write-side** concerns. A read-only connection cannot execute `PRAGMA journal_mode = WAL` — it would fail because changing journal mode requires writing to the database header. The DB file's journal mode is owned by the write pool (or by whichever process first opens the file writable).

`read_only(true)` is what makes this pool safe to size up: many concurrent readers, zero contention for the writer lock.

### Write pool: `max_connections = 1`

SQLite serializes writers at the file level. A pool of 5 writers does not get you 5× throughput — it gets you a pile of `SQLITE_BUSY` errors as connections fight for the reserved lock. With `max_connections = 1`, sqlx queues writes inside the pool (where you have backpressure and fairness) instead of pushing them into SQLite (where you get busy-loops).

### `WAL` + `synchronous = NORMAL`

- `WAL` lets readers keep reading while a writer is active. Without it, every write blocks every reader.
- `synchronous = NORMAL` is the recommended pairing with WAL — `FULL` is overkill (it fsyncs after every transaction; WAL already syncs at checkpoints) and `OFF` risks DB corruption on crash. `NORMAL` is durable across application crashes and durable enough across OS crashes for almost every workload.

### `busy_timeout = 5000ms`

If a write does collide (e.g. another process), SQLite will retry for up to 5s before returning `SQLITE_BUSY`. Without this, you get immediate failures. 5s is a reasonable wall: long enough to absorb checkpoint stalls, short enough that a true deadlock surfaces.

### `cache_size = -20000`

Negative means "kilobytes" (positive means "pages"). `-20000` = 20 MB of page cache per connection. Tune up for read-heavy workloads with hot indexes, down on memory-constrained hosts.

### `foreign_keys = true`

SQLite ships with FK enforcement **off** by default for historical reasons. Always turn it on. Without it, `ON DELETE CASCADE` and FK constraints are silently ignored.

### `temp_store = memory`

Keeps temp B-trees (used by `ORDER BY` without an index, `DISTINCT`, large `GROUP BY`) in RAM rather than spilling to a temp file.

## Writing transactions correctly

Always use `BEGIN IMMEDIATE` for any transaction that will write, even if the first statement is a `SELECT`:

```rust
let mut tx = write_pool.begin().await?;
sqlx::query("BEGIN IMMEDIATE").execute(&mut *tx).await?; // grab reserved lock now
// ... reads and writes ...
tx.commit().await?;
```

Why: SQLite's default `BEGIN` is `DEFERRED`. It starts as a reader and tries to upgrade to a writer on the first `INSERT`/`UPDATE`/`DELETE`. If another writer slipped in between, the upgrade fails with `SQLITE_BUSY` — and the busy-timeout retry **does not help here** (it only helps when acquiring a lock, not when upgrading from shared to reserved). `BEGIN IMMEDIATE` grabs the reserved lock up front; the busy-timeout then covers the wait.

## Anti-patterns to flag

- **Setting pragmas via `pool.execute("PRAGMA ...")`** — lost on replacement connections. Move them into `SqliteConnectOptions` via `.pragma(...)` or the typed builders.
- **`journal_mode(Wal)` on the read pool** — will error on connect, or silently no-op depending on sqlx version. Read-only pools must not set `journal_mode`.
- **Write pool with `max_connections > 1`** — produces `SQLITE_BUSY` under load. Always 1.
- **Plain `begin()` for write transactions** — defaults to `DEFERRED`. Upgrade-time `BUSY` is not retried by `busy_timeout`.
- **Missing `foreign_keys(true)`** — silently disables FK constraints. Always include.
- **No `busy_timeout`** — immediate `BUSY` errors instead of waiting through transient contention.
- **`synchronous = OFF`** — DB corruption risk on crash. Use `NORMAL` with WAL.
- **`synchronous = FULL` with WAL** — performance regression for no durability gain in typical workloads.
- **Setting `journal_mode = WAL` on every connect after the first** — it's a persistent setting on the DB file; setting it once is enough. sqlx will re-issue it harmlessly, but app code should not.

## Tracing

Pool creation logs at `info!`. SQL statements are logged at `debug!` via `log_statements(LevelFilter::Debug)` — keep your `RUST_LOG` / `EnvFilter` at `debug` for the relevant module if you want to see queries during local development. For production, leave statements at `debug` so they're filtered out by the default `info` level.

See the `tracing-logging` skill for the broader logging conventions.
