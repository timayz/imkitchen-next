---
name: event-sourcing
description: Event Sourcing patterns — event design, naming, schema evolution (additive, weak schema, upcasting), snapshots, replay, optimistic concurrency, idempotency, and anti-patterns. Tied to evento's executor, projection, and snapshot model.
type: reference
---

# Event Sourcing

Persist *what happened*, not the *current state*. State is a derived view computed by folding the event log. This trades complexity at write time (you have to design events well) for capabilities at read time (audit, time-travel, multiple read models, free history).

## The contract

An event store guarantees:

1. **Append-only**. You don't update or delete events. (Compaction/archival happens at a different layer.)
2. **Ordered per aggregate**. Events for one `aggregator_id` are totally ordered by `version` (1, 2, 3, …).
3. **Atomic batch per commit**. One `commit()` writes N events at consecutive versions, or none.
4. **Optimistic concurrency**. A commit specifies the expected current version (`original_version`); mismatches fail.

`evento` provides all four. The rest of this file is about *what to put in them*.

## Event design rules

### 1. Past tense, business-meaningful

| Good | Bad | Why bad |
|------|-----|---------|
| `OrderPlaced` | `PlaceOrder` | That's a command |
| `MoneyDeposited` | `DepositRequest` | Request implies it might not have happened — it did |
| `EmailAddressChanged` | `CustomerUpdated` | CRUD-shaped; loses the *what* |
| `OrderShipped` | `OrderStatusChanged { from: "packed", to: "shipped" }` | The latter encodes the read model, not the fact |

The test: if you read the event name aloud at a standup, would a non-engineer understand it? `OrderPlaced` — yes. `OrderUpdated` — no.

### 2. Small, focused

One event = one fact. If you find a single event with three optional fields, you probably have three events:

```rust
// Bad
pub enum Customer {
    Updated {
        new_name:    Option<String>,
        new_email:   Option<String>,
        new_phone:   Option<String>,
    },
}

// Good
pub enum Customer {
    NameChanged  { new_name: String },
    EmailChanged { new_email: String },
    PhoneChanged { new_phone: String },
}
```

The good version is easier to react to (you can subscribe to `EmailChanged` to send a confirmation), easier to audit, and easier to evolve.

### 3. Self-contained

An event must carry enough data to make sense in isolation. A subscriber projecting `OrderShipped` should not have to load five other things to know what shipped:

```rust
// Bad — requires the subscriber to load order details elsewhere.
pub struct OrderShipped { pub order_id: String }

// Good — carries what downstream consumers will want.
pub struct OrderShipped {
    pub order_id:     String,    // implicit via aggregator_id, but copy if useful for cross-context consumers
    pub carrier:      String,
    pub tracking_no:  String,
    pub shipped_at:   i64,
    pub line_skus:    Vec<String>,  // snapshot — if order changes later, this event still tells the truth
}
```

But don't go to the other extreme — events are not "the entire state at the time of the event." They are the *delta plus enough context*.

### 4. No queries inside event payloads

An event records what happened; it does not contain references that require live lookups. Don't store `customer: Arc<CustomerView>` — store `customer_id: String`.

### 5. No domain logic in event constructors

Events are dumb data. Validation lives in the command handler (before `commit()`), not in the event. If `MoneyDeposited` "shouldn't" have a negative amount, the handler rejects the command; the event itself doesn't enforce it.

## Naming conventions

Stable conventions reduce friction:

- `<Entity><PastTenseVerb>`: `OrderPlaced`, `PaymentCaptured`, `SubscriptionRenewed`
- For state transitions, name the *transition*, not the new state: `OrderShipped`, not `OrderStatusBecameShipped`
- For corrections / compensations, be explicit: `RefundIssued`, `OrderCancelled`, `ChargeReversed`

With `evento`, the event's persisted name is the variant name (verbatim). Renaming the variant *breaks* old reads — pick names you can live with.

## Schema evolution

Events live forever. Their shape will need to change. Strategies, in order of preference:

### A. Additive change (free)

Adding a new optional field is safe if your serializer tolerates it.

`evento` uses **bitcode**. Bitcode is *not* tolerant of structural changes by default — fields must match exactly. So additive evolution requires either:

- A new event variant for the new shape (`MoneyDepositedV2`), with both variants supported in handlers; or
- A weak-schema wrapper (see C below).

In practice, **option B (new variant) is the cleanest path** in evento. Don't try to add fields to an existing event.

### B. New event variant + handler dual-support

```rust
#[evento::aggregator]
pub enum BankAccount {
    AccountOpened   { owner_id: String, initial_balance: i64 },
    MoneyDeposited  { amount: i64, transaction_id: String },
    // Added later:
    MoneyDepositedV2 { amount: i64, transaction_id: String, source_account: Option<String> },
}

// Handle both:
#[evento::handler]
async fn on_deposit_v1(e: Event<MoneyDeposited>, v: &mut AccountView) -> anyhow::Result<()> {
    v.balance += e.data.amount; Ok(())
}
#[evento::handler]
async fn on_deposit_v2(e: Event<MoneyDepositedV2>, v: &mut AccountView) -> anyhow::Result<()> {
    v.balance += e.data.amount; Ok(())
}
```

Old events stay readable forever. New writes only produce `MoneyDepositedV2`. After enough time has passed, you may *upcast* old events (see D) so handlers can drop V1 support.

### C. Weak-schema wrapper

Store events as `serde_json::Value` or a `HashMap<String, Vec<u8>>` and read with tolerant deserializers. This is the JVM/EventStoreDB style. With `evento` + bitcode, this is uncommon — bitcode's strength is its compactness and strictness, and the new-variant approach gives you the same flexibility with stronger types.

### D. Upcasting

A one-time read-side transformation: when an old event is loaded, translate it to a newer shape in-flight. With `evento`, the closest analog is *handling both variants* (B) and *eventually retiring* the old one by:

1. Running a maintenance migration that replays each affected aggregator, rewriting its events into a fresh stream of newer variants (rarely worth it unless you're closing out old events).
2. Or simply leaving both handlers around — the cost is small.

**Rule**: prefer adding new event variants over evolving old ones. Storage is cheap; deserialization breakage is not.

### E. What you cannot do

- **Delete an event from history.** You compensate it (issue an inverse event) — you don't unwrite history.
- **Reuse an event name for a different shape.** Once `MoneyDeposited` means `{ amount, transaction_id }`, it means that forever. Use `V2`/`V3` suffixes.
- **Re-key an aggregator.** The aggregator type string (`"{cargo_pkg_name}/{Enum}"`) is part of the storage key. Renaming the crate or the enum strands old events under the old key. If you must rename, replay-translate.

## Snapshots

A snapshot is a serialized projection state at a particular version. When you read an aggregate, the projection runtime:

1. Loads the latest snapshot (if any).
2. Replays events from that snapshot's version forward.

`evento` does this automatically: any `P: bitcode::Encode + bitcode::DecodeOwned + ProjectionCursor + Send + Sync` gets a blanket `Snapshot<E>` impl, backed by the executor's snapshot table.

```rust
#[evento::projection]
#[derive(Debug)]
pub struct AccountView { pub balance: i64, pub owner: String }
// → automatically snapshot-capable. No additional code needed.
```

When to snapshot:

| Symptom | Action |
|---------|--------|
| `Projection::execute` returns `Err("Too busy")` (>100 events in a single load) | Snapshots are essential |
| Reads take noticeable time on long histories | Snapshots help |
| Aggregate has < 100 lifetime events | Don't bother |

When to *invalidate* a snapshot: bump `.revision(u16)` on the projection. The snapshot's stored revision is compared to the requested one; mismatch forces a rebuild.

```rust
Projection::<_, AccountView>::new::<BankAccount>(id)
    .handler(on_account_opened())
    .revision(2)         // change in handler logic → bump → full replay
    .execute(executor).await?;
```

Snapshot anti-patterns:

- **Snapshotting too eagerly.** Every snapshot takes a write. For low-volume aggregates, the snapshot writes cost more than the replay savings.
- **Never invalidating after handler changes.** If you change `on_money_deposited` to compute interest differently, old snapshots are wrong. Bump the revision.
- **Custom `Snapshot::take_snapshot` that returns large blobs.** It blocks the load path. Keep projections small enough to serialize quickly.

## Replay

The flip side of event sourcing: you can rebuild any projection from scratch by replaying the log.

Use cases:

1. **New read model**. You decide you want orders grouped by ZIP code. Build a new projection / subscription and let it consume from cursor `None` (start of log).
2. **Bug fix in a projector**. Drop the read table, restart the subscription, let it rebuild.
3. **Schema change in the read model**. Same — drop and rebuild.

In `evento`:

- A new `SubscriptionBuilder::new("name")` starts at the beginning of the log on first run; the cursor advances as it processes.
- To force a rebuild, drop the cursor row (in the `evento_subscriptions` table) for that subscription name, *and* truncate the read table. The next start replays from zero.

Replay performance is the practical limit on event-sourced systems. If your projection takes 6 hours to rebuild, that's also your worst-case recovery time. Mitigate with:

- **Chunked subscriptions**: `.chunk_size(N)` raises throughput.
- **Routing keys**: partition events so multiple subscriptions can rebuild in parallel.
- **Snapshots on the subscription's read model**: not via `evento::Snapshot` (that's for `Projection`) but via your own periodic checkpointing of the table state.

## Optimistic concurrency

The mechanism that keeps concurrent writers from corrupting an aggregate's history.

```rust
// Reader: load with current version known.
let view = load_account(executor, id).await?.unwrap();
let v = view.version();

// Writer: declare the expected version.
match evento::aggregator(id)
    .original_version(v)
    .event(&MoneyDeposited { amount: 100, transaction_id: tx })
    .commit(executor).await
{
    Ok(_) => { /* success */ }
    Err(evento::WriteError::InvalidOriginalVersion) => {
        // Someone else won the race. Re-load and retry the whole handler.
    }
    Err(e) => return Err(e.into()),
}
```

Retry policy: re-execute the command handler from the top. Re-loading the projection picks up the other writer's events; your validation now runs against the *current* state, and you commit at the *new* version.

**Do not** automatically retry N times in a loop without bounds — a poisonous command can spin forever. A budget of 3–5 retries with exponential backoff is typical; beyond that, return `409 Conflict` to the caller.

## Idempotency

Subscriptions may deliver the same event more than once (process restart between handler completion and cursor `acknowledge`, transient failures, etc.). Every consumer must be idempotent.

### Idempotency for read models

Use UPSERT semantics on the projection table:

```rust
sqlx::query(
    "INSERT INTO orders_by_customer (order_id, customer_id, total, placed_at)
     VALUES (?, ?, ?, ?)
     ON CONFLICT (order_id) DO UPDATE SET
        customer_id = excluded.customer_id,
        total = excluded.total,
        placed_at = excluded.placed_at"
).bind(...).execute(pool).await?;
```

Or detect-and-skip:

```rust
let already_done: bool = sqlx::query_scalar(
    "SELECT 1 FROM orders_by_customer WHERE order_id = ? AND version >= ?"
).bind(&e.aggregator_id).bind(e.version as i64).fetch_optional(pool).await?.is_some();
if already_done { return Ok(()); }
```

### Idempotency for external side effects

Send-an-email handlers are not naturally idempotent — re-running re-sends. Strategies:

1. **Outbox table** — write the intent to send into a transactional table; a separate worker reads, sends, marks done.
2. **Provider-side dedup key** — most APIs accept an idempotency key (Stripe, SendGrid). Use the event's natural key: `transaction_id`, or `format!("{aggregator_id}.{version}")`.
3. **Inbox table** — record `(aggregator_id, version)` of every handled event; skip if already present.

`evento` doesn't ship an outbox/inbox abstraction — you build it on top of subscriptions. The subscription's own cursor protects in-memory work; cross-process side effects need their own dedup.

## Anti-patterns

- **Event-CRUD**. `OrderUpdated { …all 30 fields… }`. Defeats every reason to use ES. Find the actual transitions.
- **Events as commands**. `DepositMoney` as an event. Commands are intents that can be rejected; events are facts that happened. Don't blur them.
- **Events as DTOs**. Adding fields to events because "the API caller needs them in the response." Build a separate response type.
- **Events with logic**. `impl MoneyDeposited { fn validate(&self) }` — validation runs *before* the event is created, in the handler. The event itself is a fact, not a guardian.
- **Storing the current state alongside the events**. Tempting "for fast reads" — but now you have two sources of truth, and they will diverge. Use projections + snapshots.
- **One huge global event stream**. Mixing all aggregator types in one stream defeats per-aggregate concurrency. `evento` partitions by `(aggregator_type, aggregator_id)` for a reason.
- **Renaming events in place**. `MoneyDeposited` → `FundsDeposited` "to match new vocabulary" breaks every old event in storage. Add `FundsDeposited` as a new variant; leave the old one decoding.
- **Treating events as a queue**. They're a *log*, not a queue. Don't "consume and delete" — subscriptions move cursors; the log is permanent.
- **Forgetting that the read model is derived**. Code that mutates the read table directly (outside a subscription) is poisoning your replay path. The next rebuild will *erase* your direct edits.
- **Snapshotting too early or never invalidating**. Either wasted writes, or stale projections after handler changes. Bump `.revision(u16)` when handler logic changes meaning.
- **Letting `evento` aggregator type string drift**. The persisted `aggregator_type` is `"{cargo_pkg_name}/{Enum}"`. Renaming the crate strands old events. If you anticipate renames, give your events crate a stable name from day one.
