---
name: sqlx-migrator
description: Reference for the `sqlx_migrator` crate (v0.18.0) — use when writing, editing, or debugging Rust database migrations that depend on `sqlx_migrator` for PostgreSQL, MySQL, SQLite, or `Any`. Covers the `Operation` and `Migration` traits (including `app` / `name` / `parents` / `replaces` / `run_before` / `operations` / `is_atomic` / `is_virtual` / `is_destructible`), the `(app, name)` tuple shorthand for parent references, the `migration!` / `sqlite_migration!` / `postgres_migration!` / `mysql_migration!` / `any_migration!` macros, the `vec_box!` helper, the `(up_sql, down_sql)` string-tuple operation form, the `Migrator` struct (`add_migration`, `add_migrations`, `set_schema`, `set_table_prefix`), the `Plan` constructors (`apply_all` / `apply_count` / `apply_name` / `revert_all` / `revert_count` / `revert_name` / `fake`), and the built-in `MigrationCommand` clap CLI with its `apply` / `revert` / `list` / `drop` subcommands and their flags (`--app` / `--check` / `--count` / `--fake` / `--force` / `--migration` / `--plan`). Also covers extending an existing clap CLI with `MigrationCommand`, the `OldMigrator` / `Synchronize` flow for migrating from sqlx's built-in `_sqlx_migrations` table or other migrators, and feature flags (`postgres` / `sqlite` / `mysql` / `any` / `cli`). Flags common anti-patterns: forgetting `#[async_trait::async_trait]` on `Operation` impls, omitting `down` (leaves the migration irreversible), missing `is_destructible() = true` on data-loss operations, registering migrations without their parents, mismatched `(app, name)` pairs across files, and running long migrations with `is_atomic = true` on SQLite (where the whole migration holds the write lock).
disable-model-invocation: true
---

# sqlx_migrator — v0.18.0

`sqlx_migrator` writes database migrations as **Rust code** instead of `.sql` files. Migrations are structs that implement `Migration<DB>`; each migration owns one or more `Operation<DB>` values whose `up` / `down` methods run against an `&mut <DB as Database>::Connection`. A `Migrator<DB>` holds the registered migrations, plans the execution order from the dependency graph, and tracks applied state in a metadata table inside the same database.

Source: <https://github.com/iamsauravsharma/sqlx_migrator> · Docs: <https://docs.rs/sqlx_migrator/0.18.0/sqlx_migrator/>

## Install

```toml
[dependencies]
sqlx_migrator = { version = "0.18", features = ["sqlite"] } # or "postgres", "mysql", "any"
sqlx          = { version = "0.8",  features = ["runtime-tokio", "tls-rustls", "sqlite"] }
async-trait   = "0.1"
tokio         = { version = "1", features = ["rt-multi-thread", "macros"] }
```

### Feature flags

| Flag       | Purpose |
|------------|---------|
| `cli`      | Enables `MigrationCommand` (pulls in `clap`). **On by default.** |
| `postgres` | PostgreSQL backend (`sqlx/postgres`, pulls in `crc32fast` for advisory locks) |
| `sqlite`   | SQLite backend (`sqlx/sqlite`) |
| `mysql`    | MySQL backend (`sqlx/mysql`, pulls in `crc32fast`) |
| `any`      | `sqlx::Any` backend — combine with one of the concrete backends |

Disable `default-features` if you don't need the CLI: `default-features = false, features = ["sqlite"]`.

## The two traits

### `Operation<DB>` — one atomic database change

```rust
use sqlx::{Sqlite, SqliteConnection};
use sqlx_migrator::error::Error;
use sqlx_migrator::operation::Operation;

pub struct CreateUsers;

#[async_trait::async_trait]
impl Operation<Sqlite> for CreateUsers {
    async fn up(&self, conn: &mut SqliteConnection) -> Result<(), Error> {
        sqlx::query("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
            .execute(conn)
            .await?;
        Ok(())
    }

    // Default impl returns Err(Error::IrreversibleOperation). Provide `down`
    // unless the migration is genuinely one-way (e.g., data backfill you'll
    // never undo).
    async fn down(&self, conn: &mut SqliteConnection) -> Result<(), Error> {
        sqlx::query("DROP TABLE users").execute(conn).await?;
        Ok(())
    }

    // Override to `true` for DROP/DELETE/TRUNCATE etc. — the CLI will prompt
    // before running, unless `--force` or `--fake` is set.
    fn is_destructible(&self) -> bool { false }
}
```

If you need to call the connection more than once in a single `up`/`down`, reborrow as `&mut *conn`:

```rust
sqlx::query("...").execute(&mut *conn).await?;
sqlx::query("...").execute(&mut *conn).await?;
```

### `Migration<DB>` — a unit identified by `(app, name)`

```rust
use sqlx::Sqlite;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::operation::Operation;

pub struct M0001CreateUsers;

impl Migration<Sqlite> for M0001CreateUsers {
    fn app(&self)  -> &'static str { "main" }
    fn name(&self) -> &'static str { "m0001_create_users" }

    fn parents(&self) -> Vec<Box<dyn Migration<Sqlite>>> { vec![] }

    fn operations(&self) -> Vec<Box<dyn Operation<Sqlite>>> {
        vec![Box::new(CreateUsers)]
    }
}
```

Required methods: `app`, `name`, `parents`, `operations`. The pair `(app, name)` must be unique across the registered set.

Optional methods (with defaults):

| Method            | Default | Use when |
|-------------------|---------|----------|
| `replaces()`      | `vec![]` | Squashing — if any listed migration is already applied, this one is skipped; otherwise it runs and is recorded as having replaced them. |
| `run_before()`    | `vec![]` | Force this migration to run **before** the listed ones (and revert **after** them) without making the listed ones depend on this. |
| `is_atomic()`     | `true`  | Wrap all operations in a single transaction. Override to `false` for operations that can't run inside a txn (e.g., Postgres `CREATE INDEX CONCURRENTLY`, or long SQLite work where holding the write lock for the whole migration is unacceptable). |
| `is_virtual()`    | `false` | Stand-in entry that shares `(app, name)` with another migration; used for replace-graph bookkeeping. Ignored at run time. |

#### Parent references via `(app, name)` tuple

You don't have to construct the parent migration struct if you only need to name it — `(&str, &str)` implements `Migration`:

```rust
fn parents(&self) -> Vec<Box<dyn Migration<Sqlite>>> {
    vec![Box::new(("main", "m0001_create_users"))]
}
```

Use this when the parent lives in a crate you can't easily import, or when constructing the parent requires data (see "Migrations with fields" below).

## Macros

The crate ships per-backend macros that generate the `Migration` impl for you. All take the same shape:

```text
MACRO!(StructIdent, "app", "name", parents_vec, operations_vec)
```

| Macro                | Backend       |
|----------------------|---------------|
| `sqlite_migration!`  | `sqlx::Sqlite` |
| `postgres_migration!`| `sqlx::Postgres` |
| `mysql_migration!`   | `sqlx::MySql` |
| `any_migration!`     | `sqlx::Any` |
| `migration!`         | Generic — first arg is the DB type |

`vec_box!` is a `vec![Box::new(...), Box::new(...)]` shorthand.

```rust
use sqlx_migrator::{sqlite_migration, vec_box};

pub struct M0003UseMacros;

sqlite_migration!(
    M0003UseMacros,
    "main",
    "m0003_use_macros",
    vec_box![("main", "m0002_with_parents")],          // parents
    vec_box![CreateUsers, SeedAdmin]                   // operations
);
```

### String-tuple operations — skip the `Operation` impl entirely

If an operation is one SQL string up and one SQL string down, pass it as a `(up, down)` tuple inside `vec_box!` — the crate has a blanket `Operation` impl for `(&str, &str)`:

```rust
sqlite_migration!(
    M0001CreateUsers,
    "main",
    "m0001_create_users",
    vec_box![],
    vec_box![(
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
        "DROP TABLE users"
    )]
);
```

Prefer this form for trivial schema changes. Drop down to the explicit `Operation` impl when you need bound parameters, multiple statements per operation, or `is_destructible() = true`.

## Migrations with fields (parameterised operations)

A migration may carry data that the operation reads at `up` time. Construct it when registering, and reference it from parents by `(app, name)` rather than by re-constructing:

```rust
pub struct M0004Seed { pub admin_email: String }

struct M0004Op { admin_email: String }

#[async_trait::async_trait]
impl Operation<Sqlite> for M0004Op {
    async fn up(&self, conn: &mut SqliteConnection) -> Result<(), Error> {
        sqlx::query("INSERT INTO users (name) VALUES (?)")
            .bind(&self.admin_email)
            .execute(conn)
            .await?;
        Ok(())
    }
    async fn down(&self, conn: &mut SqliteConnection) -> Result<(), Error> {
        sqlx::query("DELETE FROM users WHERE name = ?")
            .bind(&self.admin_email)
            .execute(conn)
            .await?;
        Ok(())
    }
}

impl Migration<Sqlite> for M0004Seed {
    fn app(&self)  -> &'static str { "main" }
    fn name(&self) -> &'static str { "m0004_seed_admin" }
    fn parents(&self) -> Vec<Box<dyn Migration<Sqlite>>> {
        vec![Box::new(("main", "m0001_create_users"))]
    }
    fn operations(&self) -> Vec<Box<dyn Operation<Sqlite>>> {
        vec![Box::new(M0004Op { admin_email: self.admin_email.clone() })]
    }
}
```

## Registering migrations

Collect every migration in one place and register them with the migrator. `add_migrations` accepts a `Vec<Box<dyn Migration<DB>>>`; `add_migration` takes one at a time.

```rust
// src/migrations/mod.rs
use sqlx::Sqlite;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::vec_box;

mod m0001_create_users;
mod m0002_add_email;
mod m0003_use_macros;
mod m0004_seed_admin;

pub fn migrations() -> Vec<Box<dyn Migration<Sqlite>>> {
    vec_box![
        m0001_create_users::M0001CreateUsers,
        m0002_add_email::M0002AddEmail,
        m0003_use_macros::M0003UseMacros,
        m0004_seed_admin::M0004Seed { admin_email: "admin@example.com".into() },
    ]
}
```

`add_migration` / `add_migrations` return `Result<…, Error>` — they fail if two migrations share the same `(app, name)` with different definitions. They also pull in any parents/replaces/run_before transitively, so listing the leaves is enough.

## `Migrator` API

```rust
use sqlx::Sqlite;
use sqlx_migrator::migrator::Migrator;

let mut migrator = Migrator::<Sqlite>::default();
migrator.add_migrations(crate::migrations::migrations())?;

// Optional: rename / namespace the bookkeeping table.
// Both methods return Result — they validate the identifier.
migrator.set_schema(Some("app".into()))?;        // PostgreSQL only
migrator.set_table_prefix(Some("v1".into()))?;   // every backend
```

The migrator stores applied state in a table named `_sqlx_migrator_migrations` by default. `set_table_prefix("v1")` makes it `v1_sqlx_migrator_migrations`.

## Running migrations

### Programmatic — `Migrator::run(connection, &Plan)`

```rust
use sqlx_migrator::migrator::{Migrate as _, Plan};

let mut conn = pool.acquire().await?;
migrator.run(&mut *conn, &Plan::apply_all()).await?;
```

`Plan` constructors:

| Constructor                                | What it does |
|--------------------------------------------|--------------|
| `Plan::apply_all()`                        | Apply every pending migration. |
| `Plan::apply_count(n)`                     | Apply the next `n` pending migrations in dependency order. |
| `Plan::apply_name(app, &Option<name>)`     | Apply up to and including the named migration. `None` for `name` = all of that app. |
| `Plan::revert_all()`                       | Revert every applied migration. |
| `Plan::revert_count(n)`                    | Revert the last `n`. |
| `Plan::revert_name(app, &Option<name>)`    | Revert down to (and including) the named migration. |
| `.fake(true)`                              | Combinator on any plan — update the bookkeeping table *without* running `up`/`down`. Use when the schema already matches the target state and you only need the migrator to "catch up". |

### CLI — `MigrationCommand`

If you just want a migration binary, `parse_and_run` is the whole `main`:

```rust
use sqlx::Sqlite;
use sqlx_migrator::cli::MigrationCommand;
use sqlx_migrator::migrator::Migrator;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pool = sqlx::Pool::<Sqlite>::connect(&std::env::var("DATABASE_URL")?).await?;
    let mut migrator = Migrator::<Sqlite>::default();
    migrator.add_migrations(myapp::migrations::migrations())?;

    let mut conn = pool.acquire().await?;
    MigrationCommand::parse_and_run(&mut *conn, Box::new(migrator)).await?;
    Ok(())
}
```

Exposed subcommands and flags (taken from `src/cli.rs`):

| Subcommand | Flags | Notes |
|------------|-------|-------|
| `apply`    | `--app <APP>` `--check` `--count <N>` `--fake` `--force` `--migration <NAME>` `--plan` | `--check` exits non-zero if anything is pending. `--plan` only prints the plan. `--migration` requires `--app`. `--count` conflicts with `--app`. `--force` skips the prompt for destructible migrations. |
| `revert`   | `--all` `--app <APP>` `--count <N>` `--fake` `--force` `--migration <NAME>` `--plan` | Default (no flags) reverts **one** migration. `--all` reverts everything. `--count` conflicts with `--all` and `--app`. |
| `list`     | —     | Prints `ID / App / Name / Status / Applied time`. `✓` = applied, `✗` = pending, `↔` = present in DB but not in current code (orphaned). |
| `drop`     | —     | Drops the bookkeeping table. Errors out if any migrations are still applied — revert first. |

### Extending an existing clap CLI

Embed `MigrationCommand` as a subcommand and call `.run(conn, migrator)` yourself:

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    sub: Sub,
}

#[derive(Subcommand)]
enum Sub {
    /// Database migrations
    Migrate(sqlx_migrator::cli::MigrationCommand),
    Serve,
    // ...
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let pool = build_pool().await?;
    match cli.sub {
        Sub::Migrate(cmd) => {
            let mut migrator = Migrator::<Sqlite>::default();
            migrator.add_migrations(myapp::migrations::migrations())?;
            let mut conn = pool.acquire().await?;
            cmd.run(&mut *conn, Box::new(migrator)).await?;
        }
        Sub::Serve => { /* ... */ }
    }
    Ok(())
}
```

## Migrating off `sqlx::migrate!` (or another migrator)

If you already shipped a database with sqlx's built-in `_sqlx_migrations` table (or anything else), implement `OldMigrator` and call `migrator.sync(...)` **before** `migrator.run(...)`. `sync` walks the old table and inserts equivalent rows into the `sqlx_migrator` bookkeeping table; from then on you only use `sqlx_migrator`.

```rust
use sqlx::{Database, Sqlite};
use sqlx_migrator::error::Error;
use sqlx_migrator::migration::Migration;
use sqlx_migrator::sync::OldMigrator;

struct FromSqlxMigrate;

#[async_trait::async_trait]
impl OldMigrator<Sqlite> for FromSqlxMigrate {
    async fn applied_migrations(
        &self,
        conn: &mut <Sqlite as Database>::Connection,
    ) -> Result<Vec<Box<dyn Migration<Sqlite>>>, Error> {
        let rows = sqlx::query!(
            "SELECT version, description FROM _sqlx_migrations ORDER BY version"
        )
        .fetch_all(conn)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                // Decide your mapping — version, description, or a combined key.
                let name = format!("{}_{}", r.version, r.description);
                Box::new(("main", name)) as Box<dyn Migration<Sqlite>>
            })
            .collect())
    }
}

// Then, in your migration binary, *before* running:
let old = FromSqlxMigrate;
migrator.sync(&mut *conn, &old).await?;
migrator.run(&mut *conn, &Plan::apply_all()).await?;
```

For each entry the old migrator returns, the corresponding `sqlx_migrator` migration must exist in the registered set with the **same** `(app, name)`. Otherwise the post-sync `run` will see those rows as "applied but unknown" (the `↔` orphan state) and refuse to plan around them.

## Suggested project layout

```
src/
├── lib.rs
├── migrations/
│   ├── mod.rs                       # pub fn migrations() -> Vec<Box<dyn Migration<DB>>>
│   ├── m0001_create_users.rs
│   ├── m0002_add_email.rs
│   └── m0003_seed_admin.rs
└── bin/
    └── migrate.rs                   # MigrationCommand::parse_and_run(...)
```

Filenames `mNNNN_short_name.rs` make `git log -- src/migrations/` chronological at a glance and let `(app, name)` strings match the filename exactly — easier to grep.

## Pairing with `sqlite-init`

For SQLite projects in this codebase, run migrations on a pool built by `sqlite-init`'s `create_pool` (or `create_write_pool`) — see that skill. The migrator only needs one connection; `acquire()` from either pool works. Don't run migrations on the read pool (`read_only(true)` will reject `CREATE TABLE`).

## Anti-patterns to flag

- **`impl Operation` without `#[async_trait::async_trait]`** — won't compile, but the error blames `async fn` in a trait. The attribute is required on every `Operation` impl.
- **Missing `down`** — the default `Operation::down` returns `Error::IrreversibleOperation`, so `revert` will fail on this migration. Only omit `down` when the operation is genuinely one-way and you've documented that in a comment.
- **Destructive operation without `is_destructible() = true`** — `DROP TABLE`, `DROP COLUMN`, `DELETE` without a where clause, `TRUNCATE`. Mark them so the CLI prompts before applying without `--force`.
- **Registering only the latest migration** — `add_migration(latest)` pulls in `parents()` transitively, but a missing edge in `parents()` will silently leave older migrations unregistered. Prefer registering the full list explicitly via `migrations()` so the set is auditable.
- **Mismatched `(app, name)` across files** — the string in `name()`, the string used by parents' `("main", "...")` references, and the filename should all agree. The migrator only enforces the first two; the filename is your problem.
- **Building parent migrations with `Box::new(M0001 { ... })` when M0001 has fields** — duplicating the constructor here will diverge from the registered one. Use `Box::new(("main", "m0001_xxx"))` instead.
- **`is_atomic = true` for long migrations on SQLite** — the whole migration runs inside one `BEGIN IMMEDIATE` transaction, blocking every other writer for its duration. Either split into multiple migrations or set `is_atomic = false` and handle partial-failure recovery yourself.
- **Trying to set `is_atomic = false` for `CREATE INDEX CONCURRENTLY` on Postgres but forgetting that the operation still runs inside `Operation::up`** — `CONCURRENTLY` must not be inside a transaction, so `is_atomic = false` is required *and* the migration cannot contain other operations that need a transaction.
- **Calling `migrator.run` without `Migrate as _` in scope** — `run`, `generate_migration_plan`, and friends live on the `Migrate` trait, not the inherent impl. `use sqlx_migrator::migrator::Migrate as _;` if rustc complains about an unknown method.
- **Running `Plan::apply_all().fake(true)` to "fix" a failing migration** — `fake` marks it applied without running it. Only use this when the schema *already* matches what the migration would have produced (e.g., after a manual hotfix, or during sync from another migrator). Faking past a real failure leaves the bookkeeping lying about the schema.
- **Using `MigrationCommand` with `default-features = false`** — `MigrationCommand` lives behind the `cli` feature. Either keep `cli` enabled or call `Migrator::run` directly.

## Tracing

The crate emits `tracing` spans/events for plan generation and each `up`/`down`. Enable them with the standard `EnvFilter` — see the `tracing-logging` skill.
