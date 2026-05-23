---
name: ddd-cqrs-es
description: Reference for Domain-Driven Design, CQRS, Event Sourcing, and Saga patterns — use when designing or reviewing code that talks about aggregates, bounded contexts, ubiquitous language, anti-corruption layers, commands vs queries, command handlers, projections / read models, event stores, snapshots, replay, sagas, process managers, choreography vs orchestration, compensating actions, or eventual consistency. Pairs every pattern with concrete `evento` (Rust) snippets so it composes with the `evento` skill (which documents the crate's API). Companion files in this directory: `ddd.md` (strategic + tactical DDD), `cqrs.md` (command/query split, read models, `Rw` executor), `event-sourcing.md` (event design, snapshots, replay, idempotency, schema evolution), `sagas.md` (choreography vs orchestration, process managers, compensations, retries, dedup).
disable-model-invocation: true
---

# DDD · CQRS · Event Sourcing · Sagas

This skill is **about the patterns**, not a specific library. It uses `evento` (Rust) as the concrete vehicle because that is the event-sourcing toolkit already in this workspace — if you need the crate's API surface (macros, builders, executors, subscription lifecycle, cursor pagination), consult the `evento` skill instead. This one tells you *what to build*; that one tells you *how to spell it*.

Companion references in this directory:

- [`ddd.md`](./ddd.md) — Strategic design (bounded contexts, context maps, ubiquitous language, ACL) and tactical design (entities, value objects, aggregates, domain events, repositories, factories, domain services).
- [`cqrs.md`](./cqrs.md) — Command/query separation, command handlers, validation pipelines, read models, `Rw<R, W>` split, eventual consistency budgets.
- [`event-sourcing.md`](./event-sourcing.md) — Event-as-fact, immutability, naming, schema evolution (upcasting, weak schema), snapshots, replay, optimistic concurrency, idempotency, anti-patterns.
- [`sagas.md`](./sagas.md) — Choreography vs orchestration, process managers, compensating actions, timeouts, retry, dedup, end-to-end examples on `evento` subscriptions.

## Why these patterns travel together

```
        ┌──────────────────────────────────────────────────────────┐
        │                    Bounded Context                       │
        │                                                          │
        │   Commands ──► Aggregate ──► Domain Events ──► Store     │
        │   (CQRS)       (DDD)         (ES)             (Executor) │
        │                                  │                       │
        │                                  ▼                       │
        │                          Projections / Read Models       │
        │                          (CQRS, eventually consistent)   │
        │                                  │                       │
        │                                  ▼                       │
        │                          Subscriptions / Sagas           │
        │                          (cross-aggregate workflows)     │
        └──────────────────────────────────────────────────────────┘
                                   │
                                   ▼
                  ┌──────────────────────────────┐
                  │  Other bounded contexts via  │
                  │  integration events + ACL    │
                  └──────────────────────────────┘
```

Each pattern fills one role; together they form a coherent style. Cherry-picking is fine — see "When to use what" below.

## The four patterns in one paragraph each

**DDD** is about modeling the business: discover the *ubiquitous language* with domain experts, carve the system into *bounded contexts*, and inside each context model behavior with *aggregates* that enforce invariants. Aggregates are consistency boundaries, not data structures. See [`ddd.md`](./ddd.md).

**CQRS** splits the write path (commands that mutate via aggregates) from the read path (queries that hit denormalized read models). The two sides may share a database or live on separate stores — what matters is that reads are not constrained by the write model's shape. See [`cqrs.md`](./cqrs.md).

**Event Sourcing** persists *facts about what happened* rather than current state. State is derived by folding events. Events are immutable, append-only, versioned per aggregate, and decoupled from the read model. See [`event-sourcing.md`](./event-sourcing.md).

**Sagas** coordinate long-running, cross-aggregate processes (e.g., "place order → reserve stock → charge card → ship"). They live outside any aggregate's transaction boundary and use compensating actions instead of distributed transactions. Two flavors: *choreography* (each service reacts to events) and *orchestration* (one process manager directs the dance). See [`sagas.md`](./sagas.md).

## Mapping to `evento`

| Pattern concept | `evento` construct |
|-----------------|-------------------|
| Aggregate type (DDD) | `#[evento::aggregator] enum X { … }` — variants become event structs; the enum becomes a unit-struct *handle* (e.g., `BankAccount`) |
| Aggregate ID | The ULID returned by `create().commit(...)` — or hashed from inputs via `AggregatorBuilder::ids(...)` |
| Domain event | A variant of the aggregator enum → a `bitcode`-encoded struct |
| Command handler (CQRS write side) | Plain function that loads a projection (via `Projection::new::<A>(id)`), validates, then calls `create()` / `aggregator(id).original_version(v)…commit(&executor)` |
| Read model (CQRS read side) | `#[evento::projection]` struct + `#[evento::handler]` functions, or a custom SQL table populated by a `#[evento::subscription]` |
| Eventual consistency boundary | The gap between `commit(&executor)` and the next poll of a `SubscriptionBuilder` (default ~1s) |
| Process manager / orchestrator saga | `#[evento::subscription]` handlers that emit *commands* to other aggregates, optionally backed by their own aggregate that tracks saga state |
| Choreography saga | Independent `#[evento::subscription]`s each reacting to a peer's domain event |
| Compensating action | A new event (e.g., `RefundIssued`) on the original aggregate, emitted by the saga handler |
| Anti-corruption layer | A translator function at the bounded-context boundary that maps inbound integration events into local domain events before they're committed |
| Optimistic concurrency | `aggregator(id).original_version(v)` — `WriteError::InvalidOriginalVersion` on conflict |
| Snapshots | Automatic `Snapshot<E>` blanket impl on `bitcode`-encodable projections; bump `.revision(u16)` to invalidate |

## When to use what

| If you have… | Reach for… | Skip… |
|--------------|------------|-------|
| A complex domain with experts and policies | DDD tactical patterns | Bare CRUD |
| Read and write workloads with very different shapes | CQRS (split models) | A single ORM model for both |
| Audit, time-travel, "why did this happen?", projections that change shape over time | Event Sourcing | Storing only current state |
| A workflow that spans multiple aggregates or services | Saga | A distributed transaction (2PC) |
| Cross-context integration | Bounded contexts + ACL + integration events | Sharing a database |
| Simple CRUD app, no audit needs, one team | None of the above — use a normalized DB | All of it (overkill) |

**These patterns are not all-or-nothing.** DDD without ES is normal (state-stored aggregates). CQRS without ES is normal (two projections of the same SQL data). ES without DDD is dangerous (events become a dumping ground without invariants). Sagas without ES are common (any message bus + idempotent handlers will do).

## The smallest end-to-end example

A bank account with one command (`Deposit`), one read model (`AccountView`), and one saga that posts a notification when a deposit exceeds a threshold.

```rust
// ─── DDD: one aggregator with three events ────────────────────────────
#[evento::aggregator]
pub enum BankAccount {
    AccountOpened   { owner_id: String, initial_balance: i64 },
    MoneyDeposited  { amount: i64, transaction_id: String },
    MoneyWithdrawn  { amount: i64 },
}

// ─── CQRS write side: command + handler ───────────────────────────────
pub struct Deposit { pub account_id: String, pub amount: i64, pub tx: String }

pub async fn handle_deposit<E: evento::Executor>(
    cmd: Deposit,
    executor: &E,
) -> anyhow::Result<()> {
    // Load current state to validate invariants.
    let account = evento::projection::Projection::<_, AccountView>
        ::new::<BankAccount>(&cmd.account_id)
        .handler(on_account_opened())
        .handler(on_money_deposited())
        .handler(on_money_withdrawn())
        .execute(executor).await?
        .ok_or_else(|| anyhow::anyhow!("unknown account"))?;

    anyhow::ensure!(cmd.amount > 0, "non-positive deposit");

    // Append the fact.
    evento::aggregator(&cmd.account_id)
        .original_version(account.version())          // optimistic concurrency
        .event(&MoneyDeposited { amount: cmd.amount, transaction_id: cmd.tx })
        .commit(executor).await?;
    Ok(())
}

// ─── CQRS read side: projection ───────────────────────────────────────
#[evento::projection]
#[derive(Debug)]
pub struct AccountView { pub balance: i64, pub owner: String }

#[evento::handler]
async fn on_account_opened(e: Event<AccountOpened>, v: &mut AccountView) -> anyhow::Result<()> {
    v.owner = e.data.owner_id.clone(); v.balance = e.data.initial_balance; Ok(())
}
#[evento::handler]
async fn on_money_deposited(e: Event<MoneyDeposited>, v: &mut AccountView) -> anyhow::Result<()> {
    v.balance += e.data.amount; Ok(())
}
#[evento::handler]
async fn on_money_withdrawn(e: Event<MoneyWithdrawn>, v: &mut AccountView) -> anyhow::Result<()> {
    v.balance -= e.data.amount; Ok(())
}

// ─── Saga (choreography): react to large deposits ─────────────────────
#[evento::subscription]
async fn notify_large_deposit<E: evento::Executor>(
    _ctx: &evento::subscription::Context<'_, E>,
    e: Event<MoneyDeposited>,
) -> anyhow::Result<()> {
    if e.data.amount >= 10_000 {
        // Side effect — must be idempotent on (aggregator_id, version)
        // or on transaction_id, since the subscription will retry on failure.
        send_notification(&e.aggregator_id, e.data.amount).await?;
    }
    Ok(())
}
```

What each pattern bought us:
- **DDD**: `BankAccount` is the consistency boundary; the command can't be split into two transactions that violate balance invariants.
- **CQRS**: `AccountView` is shaped for reads — we don't query the event log at request time.
- **ES**: We can re-derive `AccountView` with a different shape any time by replaying events; we have an audit trail for free.
- **Saga**: The "notify on large deposit" rule lives outside the aggregate — it's a *policy*, not an invariant. The aggregate stays focused on balance correctness.

## Common pitfalls (the cross-cutting ones)

- **Anemic aggregates**. If your aggregate is just a bag of fields with no methods, you're doing data-modeling, not DDD. Behavior belongs *on* the aggregate; events are emitted *by* it.
- **Treating events as DTOs**. Events are domain facts in past tense (`MoneyDeposited`, not `DepositMoney` or `DepositRequest`). Naming matters because events are forever.
- **Read-then-write race**. The CQRS read model lags. Never validate a command against a *projection that was built from a different cursor than the one you'll commit against*. Inside a command handler, load via `Projection::execute` and immediately commit with `original_version(v)` — `evento`'s optimistic concurrency catches the race.
- **Distributed transactions in disguise**. Reaching for a saga because "we need to update two things atomically" is the right instinct — but the *output* of a saga is a sequence of events with compensations, not a hidden 2PC. Design the compensation path *first*.
- **Mixing concerns in a subscription handler**. A subscription handler that writes a read model *and* emits commands *and* calls an external API is three sagas pretending to be one. Split them; their failure modes diverge.
- **Snapshotting too early**. Projections that read fast and rarely change don't need snapshots. Add them when `Projection::execute` starts returning `"Too busy"` (>100 events without a snapshot in a single load) — not before. See the `evento` skill for snapshot mechanics.
- **One bounded context, one event store**. Sharing one `Executor` across contexts is fine *technically* but couples deployment and schema evolution. Prefer one executor per context with explicit integration events at the boundary (see [`ddd.md`](./ddd.md)).
- **Forgetting idempotency on the read side**. Subscriptions re-deliver on retry. Every projection and saga handler must be safe to run twice on the same event. The natural dedup key is `(aggregator_id, version)`; for external side effects, use a business key (`transaction_id`).

## How to use this skill

When the user mentions an aggregate, bounded context, command, projection, saga, or compensation:

1. **Start in `SKILL.md`** for the mental model and the evento mapping.
2. **Dive into the relevant companion** for the depth needed: design questions → `ddd.md`; write/read split → `cqrs.md`; event design → `event-sourcing.md`; workflows → `sagas.md`.
3. **Cross-reference the `evento` skill** for the actual macro/builder/subscription API. This skill never duplicates that material — it always shows the pattern in evento terms and links over.
