# Executors, migrations, and raw queries

`Executor` is the storage abstraction in `evento-core`. All write/read APIs in evento are generic over it.

```rust
#[async_trait]
pub trait Executor: Send + Sync + 'static {
    async fn write(&self, events: Vec<Event>) -> Result<(), WriteError>;
    async fn read(&self, aggregators: Option<Vec<ReadAggregator>>,
                  routing_key: Option<RoutingKey>, args: Args)
        -> anyhow::Result<ReadResult<Event>>;
    async fn get_subscriber_cursor(&self, key: String) -> anyhow::Result<Option<Value>>;
    async fn is_subscriber_running(&self, key: String, worker_id: Ulid) -> anyhow::Result<bool>;
    async fn upsert_subscriber(&self, key: String, worker_id: Ulid) -> anyhow::Result<()>;
    async fn acknowledge(&self, key: String, cursor: Value, lag: u64) -> anyhow::Result<()>;
    async fn get_snapshot(&self, aggregator_type: String, revision: String, id: String)
        -> anyhow::Result<Option<(Vec<u8>, Value)>>;
    async fn save_snapshot(&self, aggregator_type: String, revision: String, id: String,
                           data: Vec<u8>, cursor: Value) -> anyhow::Result<()>;
}
```

You almost never implement this trait yourself — use one of the built-in executors:

| Type | Cargo feature | Crate |
|------|---------------|-------|
| `Sql<DB>` (`evento::Sqlite` / `MySql` / `Postgres`) | `sqlite` / `mysql` / `postgres` | `evento-sql` |
| `Fjall` | `fjall` | `evento-fjall` |
| `Evento` (type-erased `Arc<Box<dyn Executor>>`) | (always) | `evento-core` |
| `EventoGroup` | `group` | `evento-core` |
| `Rw<R, W>` | `rw` | `evento-core` |

## SQL setup

```rust
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx_migrator::{Migrate, Plan};

let pool = SqlitePoolOptions::new()
    .max_connections(8)
    .connect_with(SqliteConnectOptions::new()
        .filename("events.db")
        .create_if_missing(true))
    .await?;

// 1) Run migrations once on startup
let mut conn = pool.acquire().await?;
evento::sql_migrator::new::<sqlx::Sqlite>()?
    .run(&mut *conn, &Plan::apply_all())
    .await?;
drop(conn);

// 2) Wrap the pool — `From<Pool<DB>> for Sql<DB>` is provided
let executor: evento::Sqlite = pool.into();
```

For Postgres / MySQL, swap the generic parameter and pool type:

```rust
let executor: evento::Postgres = pg_pool.into();
let executor: evento::MySql   = my_pool.into();
```

`Sql<DB>` is `Clone` (cheap; just clones the inner sqlx `Pool` which is `Arc`-shared).

### Migrations

`evento_sql_migrator::new::<DB>()` returns a configured `sqlx_migrator::Migrator<DB>` with three migrations registered:

| Migration | Adds |
|-----------|------|
| `InitMigration` | `event`, `snapshot`, `subscriber` tables |
| `M0002` | `event.timestamp_subsec` column |
| `M0003` | Drops `snapshot` table; widens `event.name` |

Tables after applying all:

**`event`** — `id VARCHAR(26)`, `name VARCHAR(50)`, `aggregator_type VARCHAR(50)`, `aggregator_id VARCHAR(26)`, `version INTEGER`, `data BLOB`, `metadata BLOB`, `routing_key VARCHAR(50)`, `timestamp BIGINT`, `timestamp_subsec BIGINT`.

**`subscriber`** — `key VARCHAR(50) PRIMARY KEY`, `worker_id VARCHAR(26)`, `cursor TEXT`, `lag INTEGER`, `enabled BOOLEAN`, `created_at TIMESTAMP`, `updated_at TIMESTAMP`.

Snapshots (after M0003) are stored via the executor's `get_snapshot` / `save_snapshot` methods — backends choose their own physical storage; the public `Snapshot` trait abstracts it.

## Fjall (embedded)

```rust
use evento_fjall::Fjall;

let executor: Fjall = Fjall::open("./events")?;     // creates directories if missing
// or, custom keyspace:
let keyspace = fjall::Config::new("./events")
    .max_write_buffer_size(128 * 1024 * 1024)
    .open()?;
let executor = Fjall::from_keyspace(keyspace)?;

executor.persist()?;                                 // force fsync
```

No migrations — Fjall creates its partitions on first open. Data is laid out across five partitions: `events`, `agg_index`, `routing_index`, `type_index`, `subscribers`.

## Type-erased / composite executors

### `Evento`

`evento::Evento::new(executor)` wraps any `Executor` in `Arc<Box<dyn Executor>>`. Use it when you want one type that can hold any backend (e.g., behind a trait object, in `axum::State`).

```rust
let any: evento::Evento = sqlite_executor.into();    // also: From<Sqlite> / From<&Sqlite> etc.
```

### `EventoGroup` (feature `group`)

Aggregates multiple `Evento` executors. Reads fan out and merge by cursor; writes go to the first executor only. Used for read-only aggregation across multiple stores.

```rust
let group = evento::EventoGroup::default()
    .executor(primary)
    .executor(secondary);
```

### `Rw<R, W>` (feature `rw`)

Read/write split. Reads, snapshots, and subscriber-cursor reads go to `R`; writes, snapshot saves, `upsert_subscriber`, and `acknowledge` go to `W`. Construct via `From<(R, W)>`:

```rust
let rw: evento::sql::RwSqlite = (read_pool.into(), write_pool.into()).into();
```

`RwSqlite` / `RwMySql` / `RwPostgres` are convenience aliases (`Rw<Sqlite, Sqlite>`, etc.).

## Raw paginated reads with `Reader` (SQL)

For custom read-models, use `evento::sql::Reader` — a thin wrapper over a sea-query `SelectStatement` that adds cursor-based pagination.

```rust
use evento::{
    sql::Reader,
    cursor::{Args, ReadResult},
};
use sea_query::{Expr, Query};

// Build a sea-query statement against your own table
let stmt = Query::select()
    .columns([Account::Id, Account::Owner, Account::Balance, Account::CreatedAt])
    .from(Account::Table)
    .and_where(Expr::col(Account::Status).eq("active"))
    .to_owned();

let result: ReadResult<AccountRow> = Reader::new(stmt)
    .forward(20, None)                  // .forward(n, after_cursor)
    .execute::<sqlx::Sqlite, AccountRow, _>(&pool)
    .await?;
```

Requirements on `AccountRow`:

- `sqlx::FromRow` for your DB.
- `evento::cursor::Cursor + evento::sql::Bind<Cursor = Self>` — usually generated by `#[derive(evento::Cursor)]`. See [`macros.md`](./macros.md) for the cursor derive.

`Reader` impls `Deref<Target = SelectStatement>` / `DerefMut`, so you can keep mutating the underlying query before `execute`:

```rust
let mut r = Reader::new(stmt);
r.and_where(Expr::col(Account::Currency).eq("USD"))
 .forward(20, after);
let page = r.execute::<sqlx::Sqlite, AccountRow, _>(&pool).await?;
```

`.desc()` / `.order(cursor::Order::Asc | Desc)` controls direction.

## `ReadAggregator` filter

```rust
ReadAggregator::aggregator("crate/BankAccount")              // all events of a type
ReadAggregator::id("crate/BankAccount", account_id)          // one instance
ReadAggregator::event("crate/BankAccount", "MoneyDeposited") // one event name across instances
ReadAggregator::new("crate/BankAccount", account_id, "Deposit") // full filter
```

Pass as `Some(vec![...])` to `Executor::read`. Multiple filters are OR'd; fields within one filter are AND'd. `None` means no aggregator filter.

## Snapshots

When `P: bitcode::Encode + bitcode::DecodeOwned + ProjectionCursor + Send + Sync`, evento auto-implements `Snapshot<E>` using the executor's `get_snapshot` / `save_snapshot`. `Projection::execute`:

1. Calls `P::restore(&context)` — usually returns the persisted snapshot.
2. Reads events forward from the snapshot cursor (or from the beginning).
3. Applies each event via the matching `Handler`.
4. If any events were applied, calls `snapshot.take_snapshot(&context)` with the last cursor.

To use custom snapshot storage (e.g., a Redis cache or an in-memory `HashMap` for tests), implement `Snapshot<E>` for your projection type *manually* — that overrides the blanket impl. The `bank` example does this with `once_cell::Lazy<RwLock<HashMap<...>>>`.

`Projection::revision(u16)` participates in the snapshot key. Bumping the revision invalidates all existing snapshots and forces a rebuild — use this when the projection's shape changes incompatibly.
