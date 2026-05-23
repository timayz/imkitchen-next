---
name: cqrs
description: Command/query responsibility segregation — command shape and validation, command handlers, read models, eventual consistency, projections vs query handlers, and the evento `Rw<R, W>` split executor.
type: reference
---

# CQRS — Command/Query Responsibility Segregation

CQRS is a *write-vs-read split*. Writes go through commands handled by aggregates; reads go through query models shaped for the consumer. Nothing more, nothing less.

It is **not** the same as event sourcing — you can do CQRS with two SQL views over the same database. But CQRS and ES amplify each other: ES naturally produces multiple read models from one event stream, and CQRS gives you a clean place to put them.

## The two sides

```
                 ┌────────────────┐
   Command ────► │ Command Handler│ ──► Aggregate.commit() ──► Event Store
                 └────────────────┘                                  │
                                                                     ▼
                                                            ┌─────────────────┐
                                                            │  Projections /  │
                                                            │  Read models    │
                                                            └─────────────────┘
                                                                     ▲
                                                                     │
                  Query ──► Query Handler ───────────────────────────┘
                            (just reads — no domain logic)
```

The asymmetry matters:

- **Writes are transactional** — one aggregate, one commit, one optimistic-concurrency check.
- **Reads are eventually consistent** — they see the world as of the last subscription poll (`evento` polls ~1s).
- **Writes are normalized** around invariants; reads are denormalized around screens and queries.

## Commands

A command is **an intent** to change state. Imperative tense. Usually addressed to a specific aggregate.

| Good | Bad |
|------|-----|
| `Deposit { account_id, amount }` | `DepositMoney` (verb-noun ambiguity) |
| `CancelOrder { order_id, reason }` | `UpdateOrderStatus { status: "cancelled" }` |
| `ShipOrder { order_id, carrier }` | `OrderUpdater { … }` |

Rules:

- **Commands can be rejected.** They fail validation, hit invariants, lose optimistic-concurrency races. Plan for failure.
- **Commands have no side effects on success but the events they emit.** A command handler that posts to a webhook *and* commits events has a saga inside it (badly) — see [`sagas.md`](./sagas.md).
- **Commands are not events.** `OrderShipped` (past tense, fact) is an event; `ShipOrder` (imperative, intent) is a command. Do not persist commands. The event is what's worth keeping.

In Rust, commands are typically plain structs with no behavior:

```rust
pub struct PlaceOrder {
    pub customer_id: String,
    pub lines: Vec<LineItem>,
}

pub struct CancelOrder {
    pub order_id: String,
    pub reason: String,
}
```

### Command handlers

A command handler:

1. **Loads** the current state of the target aggregate (via a projection).
2. **Validates** the command against current state (invariants, business rules).
3. **Commits** new events with optimistic concurrency.
4. **Returns** an outcome (id, error). It does *not* return the new state.

```rust
pub async fn place_order<E: evento::Executor>(
    executor: &E, cmd: PlaceOrder,
) -> anyhow::Result<String> {
    // 1) Validate at command-shape level (cheap, stateless).
    anyhow::ensure!(!cmd.lines.is_empty(), "empty order");
    anyhow::ensure!(cmd.lines.iter().all(|l| l.qty > 0), "non-positive qty");

    // 2) Optional: validate against external state.
    // (We don't load the customer here — that's a different aggregate.
    // If we need to assert "customer is active", read its projection.)

    // 3) Commit.
    let order_id = evento::create()
        .event(&OrderPlaced { customer_id: cmd.customer_id, lines: cmd.lines })
        .commit(executor).await?;
    Ok(order_id)
}

pub async fn cancel_order<E: evento::Executor>(
    executor: &E, cmd: CancelOrder,
) -> anyhow::Result<()> {
    // 1) Load.
    let order = load_order(executor, &cmd.order_id).await?
        .ok_or_else(|| anyhow::anyhow!("unknown order"))?;

    // 2) Validate against current state.
    anyhow::ensure!(!order.is_cancelled(), "already cancelled");
    anyhow::ensure!(!order.is_shipped(),   "already shipped — issue a return instead");

    // 3) Commit, with the version we just read.
    evento::aggregator(&cmd.order_id)
        .original_version(order.version())
        .event(&OrderCancelled { reason: cmd.reason })
        .commit(executor).await?;
    Ok(())
}
```

The **load → validate → commit** triple is the load-bearing pattern. `evento::aggregator(id).original_version(v)…commit()` makes the race safe: if another command commits between our load and our commit, `WriteError::InvalidOriginalVersion` fires and we retry the handler from step 1.

### Command validation: where does it live?

Three tiers:

| Tier | Example | Where |
|------|---------|-------|
| Shape | `qty > 0`, non-empty string, valid email format | Command struct's constructor or first lines of the handler |
| Cross-aggregate state | "the customer is active" | Read a projection of the other aggregate (eventually consistent — fine for most rules) |
| Aggregate invariant | "balance stays ≥ 0", "no double-cancel" | Inside the handler, after loading the target aggregate, before commit |

The last tier is the only one that needs optimistic concurrency — it's the one that races. Shape and cross-aggregate checks are advisory; the aggregate is the final authority on its own invariants.

## Queries and read models

A query is a request for a *view* of state. Queries:

- Are read-only — no commits, no side effects.
- Are eventually consistent — they see what the projection has caught up to.
- Are shaped for the consumer — joins, denormalizations, aggregations baked in.

### Two ways to build a read model in `evento`

**A) Per-aggregate `Projection`** — replay-on-demand. Good for command handlers that need the current state of *one* aggregate.

```rust
#[evento::projection]
pub struct OrderView {
    pub status: OrderStatus,
    pub total:  Money,
    pub lines:  Vec<LineItem>,
}

pub async fn load_order<E: evento::Executor>(
    executor: &E, id: &str,
) -> anyhow::Result<Option<OrderView>> {
    evento::projection::Projection::<_, OrderView>
        ::new::<Order>(id)
        .handler(on_order_placed())
        .handler(on_line_item_added())
        .handler(on_order_submitted())
        .handler(on_order_cancelled())
        .execute(executor).await
}
```

This recomputes from events every call (with snapshot acceleration). Great for command handlers, **wrong** for "list all orders for a customer" — that would require reading *every* aggregate.

**B) Subscription-backed SQL table** — eagerly maintained denormalized table. Good for list/search queries.

```rust
// Subscription updates a Postgres table that you query directly.
#[evento::subscription]
async fn project_orders_by_customer<E: evento::Executor>(
    ctx: &evento::subscription::Context<'_, E>,
    e: Event<OrderPlaced>,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO orders_by_customer (order_id, customer_id, total, placed_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT (order_id) DO NOTHING"
    )
    .bind(&e.aggregator_id)
    .bind(&e.data.customer_id)
    .bind(compute_total(&e.data.lines))
    .bind(e.created_at)
    .execute(ctx.data::<sqlx::SqlitePool>().unwrap()).await?;
    Ok(())
}

// And queries against that table — plain SQL.
pub async fn list_orders_for_customer(
    pool: &sqlx::SqlitePool, customer_id: &str,
) -> anyhow::Result<Vec<OrderRow>> {
    let rows = sqlx::query_as::<_, OrderRow>(
        "SELECT * FROM orders_by_customer WHERE customer_id = ? ORDER BY placed_at DESC"
    )
    .bind(customer_id)
    .fetch_all(pool).await?;
    Ok(rows)
}
```

**Rule**: ON CONFLICT DO NOTHING (or equivalent UPSERT) on the insert path makes the projection idempotent under retry — subscriptions can deliver the same event more than once. See [`event-sourcing.md`](./event-sourcing.md) on idempotency.

### When to use which

| Need | Use |
|------|-----|
| Command handler validating one aggregate | `Projection::execute` |
| One-aggregate API read endpoint with low traffic | `Projection::execute` |
| One-aggregate API read endpoint with high traffic | Subscription-backed table |
| List/search across aggregates | Subscription-backed table |
| Aggregations (sums, counts, percentiles) | Subscription-backed table |
| Read model that occasionally needs reshape | Subscription-backed table (use `.revision(u16)` on a Projection too) |

## Eventual consistency

The read side lags the write side by however long it takes the subscription to poll and apply. With `evento` defaults that's ~1s under steady state and can spike under load.

Make this **explicit**, not hidden:

- API responses to commands return the new aggregate's ID, not its new state. If the client wants the new state, it queries — and learns it may not see its own write yet.
- For a "read your write" UX, the client passes the aggregate version it expects to see, and the API blocks (briefly) until the read model has caught up, or echoes back the version it served.
- Don't validate one command using a read model that another command just wrote — the lag is real, and you'll occasionally accept commands that violate invariants. Validate inside the same aggregate.

### Cross-aggregate validation under eventual consistency

Sometimes you genuinely need to validate one aggregate's command against another aggregate's state:

> "Allow `PlaceOrder` only if `Customer` is not blacklisted."

You have three honest options:

1. **Eventually consistent check** — read `CustomerView` projection. Cheap, but a `Customer` blacklisted *just now* might still place an order. Acceptable for most business rules.
2. **Merge the aggregates** — if the invariant is non-negotiable, the two aggregates are one. Customers' blacklist status is an invariant on `Customer` and a precondition on `Order` → put both under one aggregate, or push the check into a saga that *cancels* the order after the fact.
3. **Saga with compensation** — accept the order, then a saga reads customer state and emits `OrderCancelled { reason: BlacklistedCustomer }` if needed. See [`sagas.md`](./sagas.md).

There is no fourth option. "Use a distributed transaction" is not it — that's solving the wrong problem.

## Splitting reads and writes physically: `Rw<R, W>`

`evento::Rw<R, W>` lets you direct writes to one executor and reads to another:

```rust
let write_pool = sqlx::PgPool::connect("postgres://primary/…").await?;
let read_pool  = sqlx::PgPool::connect("postgres://replica/…").await?;

let executor: evento::Rw<evento::Postgres, evento::Postgres> =
    evento::Rw::new(write_pool.into(), read_pool.into());
```

Useful when:

- The replica is geographically closer or scaled separately.
- You want to keep read-heavy projections from competing with write locks.

Caveats:

- Replica lag is now *visible* to your read models. Their `acknowledge` cursor advances against the replica's clock.
- Write-then-immediate-read on the same logical query can return stale results — wider than the normal projection lag.

If you just want to scale reads, `EventoGroup` (read from any of N executors, write to the first) is often closer to what people actually want.

## Command/query handler signatures — a sane shape

A simple, idiomatic pattern that scales from one command to dozens, without inventing an enum dispatcher or a "bus":

```rust
// Each command is its own struct.
// Each handler is its own async fn taking (executor, cmd) -> Result.
// Wire them up at the HTTP/RPC boundary.

#[derive(Debug)]
pub struct PlaceOrder { /* … */ }

pub async fn place_order<E: evento::Executor>(
    executor: &E, cmd: PlaceOrder,
) -> anyhow::Result<String> { /* … */ }

// HTTP boundary (axum-style):
async fn place_order_endpoint(
    State(executor): State<evento::Sqlite>,
    Json(cmd): Json<PlaceOrder>,
) -> impl IntoResponse { /* call handler */ }
```

You don't need a "command bus" or "mediator". Direct function calls are easier to test, easier to read, and fully type-checked. Reach for a bus only if you have a *concrete* need (e.g. instrumentation around every command), and then make it a thin generic wrapper, not a framework.

## Common pitfalls

- **Treating the read model as authoritative**. The event store is authoritative. A read model is a cache. If you find yourself validating a *write* against a *read model alone*, you've inverted authority — load the aggregate via a projection in the handler instead.
- **Returning the new state from a command handler**. The handler returns an ID (and maybe a version). If you compute the new state and return it, you'll be tempted to skip the projection — but other consumers still need it, and you've now built two ways to "get the state of an order".
- **Synchronous projections in the request path**. Writing to a `subscription`-backed table inside the command handler (before `commit()`) breaks the abstraction — the table now races with the subscription. Pick one.
- **Eventually-consistent invariants**. "No two users can register with the same email" using a read model that lags will occasionally accept duplicates. Either enforce uniqueness in a database constraint on the projection (and reconcile by compensation), or push the email into an aggregate whose ID *is* the email (so the second `create()` collides).
- **Forgetting `.original_version(v)`**. Without it, two concurrent commands silently overwrite each other's view of the world. The default is `0`, which is correct only for the very first commit. Use the version you just read.
- **Building a "command bus" before you need one**. Direct function calls are faster, simpler, and clearer. Defer the bus until you have an actual need that justifies it (cross-cutting auth, metrics, etc.) and even then a generic `dispatch<C>(cmd: C)` is plenty.
- **Sharing one projection between command-handler-load and query-endpoint**. Tempting (DRY!), but they have different requirements: handlers need *strong* consistency (latest version) and query endpoints want *eventually* consistent denormalized shape. Two projections is usually right.
