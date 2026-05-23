# Subscriptions

`SubscriptionBuilder<E>` configures a long-running task that polls events from an executor, dispatches them to registered handlers, and acknowledges cursor progress per event.

## Builder API

```rust
use std::time::Duration;
use evento::{
    Executor,
    metadata::Event,
    subscription::{Context, SubscriptionBuilder, Subscription},
};

let sub: Subscription = SubscriptionBuilder::<evento::Sqlite>::new("deposit-notifier")
    .handler(on_deposit())            // register handlers (one per event type)
    .handler(on_withdraw())
    .skip::<AccountClosed>()          // optional: explicit no-op
    .data(my_shared_state)            // optional: injected into handler ctx
    .routing_key("accounts")          // optional: only events with this key
    .chunk_size(100)                  // batch size; default 300
    .retry(5)                         // exponential backoff retries; default 30
    .delay(Duration::from_secs(10))   // delay before first poll
    .accept_failure()                 // continue past handler errors instead of stopping
    .safety_check()                   // fail if an event has no handler
    .start(&executor)                 // spawn background task
    .await?;

// later
sub.shutdown().await?;                // graceful: signals shutdown, awaits in-flight event
```

Alternative entry points:

- `.unretry_start(&executor)` — same as `start` but with `retry` disabled.
- `.execute(&executor)` — process all pending events once and return; does not spawn.
- `.unretry_execute(&executor)` — `execute` with no retries.

`Subscription { id: Ulid, … }`. The `id` is the worker id stored in the `subscriber` row.

## Routing keys

```rust
pub enum RoutingKey {
    All,                       // match all events regardless of key
    Value(Option<String>),     // match events whose routing_key equals this (None = unkeyed)
}
```

`SubscriptionBuilder` defaults to `RoutingKey::Value(None)` — meaning *only events with no routing key* are processed. To consume everything use `.all()`. To consume one partition, use `.routing_key("accounts")`.

Setting a routing key also changes the subscription's persistence key from `"my-sub"` to `"accounts.my-sub"`, so two subscriptions with the same logical key but different routing keys keep separate cursors. This is the standard pattern for parallel partitioned consumers.

## Handler dispatch

For each event in a chunk, the runtime computes two keys:

```
all_key = "{aggregator_type}_all"
key     = "{aggregator_type}_{event_name}"
```

It looks up `all_key` first, then `key`. Whichever matches first runs. So a `#[evento::subscription_all]` handler shadows per-event handlers on the same builder.

Multiple handlers for the same `(aggregator_type, event_name)` are rejected at registration: `SubscriptionBuilder::handler` panics with `"Cannot register event handler: key {…} already exists"`.

`safety_check()` flips an internal flag — when an event has no matching handler:

- disabled (default): event is skipped, cursor advances.
- enabled: `process` bails with `anyhow!("no handler …")`.

## Error handling and retries

`SubscriptionBuilder::retry(n)` wraps the per-poll `process` call in `backon::ExponentialBuilder::default().with_max_times(n)`. If processing fails (an event handler returns `Err`):

- Each retry: re-runs the entire batch from the current cursor.
- After all retries exhaust: `accept_failure()` → log + continue to the next poll cycle; otherwise the task exits.

Be aware that retries replay the events that haven't been acknowledged yet. Handlers must therefore be idempotent (or use the cursor/`acknowledge` semantics carefully).

## Polling cadence and acknowledgement

Internally the task ticks ~once per second (`interval_at(... 1000ms)`), and inside `process` it polls in a tighter loop at ~300ms while it has events. For each event:

1. Read the latest event timestamp (for lag calculation).
2. Read `chunk_size` events forward from the current cursor.
3. For each event:
   - Check shutdown signal (drop the lock, exit if signalled).
   - Run the handler.
   - `executor.acknowledge(key, event.cursor, lag)` — persists the new cursor and lag.

`lag` is `latest_timestamp - this_event.timestamp` in seconds, saturating at 0.

## Context API

```rust
pub struct Context<'a, E: Executor> {
    pub executor: &'a E,
    /* deref target: RwContext */
}
```

Inside a handler:

```rust
#[evento::subscription]
async fn on_deposit<E: Executor>(
    ctx: &Context<'_, E>,
    event: Event<MoneyDeposited>,
) -> anyhow::Result<()> {
    // 1) executor for downstream queries
    let history = ctx.executor.read(
        Some(vec![ReadAggregator::id("crate/BankAccount", &event.aggregator_id)]),
        None,
        Args::forward(50, None),
    ).await?;

    // 2) shared data (must be Clone for RwContext::extract)
    let config: AppConfig = ctx.extract();        // panics if not injected

    // 3) optional non-panicking accessors:
    if let Some(cfg) = ctx.get::<AppConfig>() { … }

    Ok(())
}
```

Inject data with `.data(value)` on the builder. Values are keyed by `TypeId` — only one instance per type at a time. Wrap in `evento::context::Data<T>` (an `Arc`) when you want cheap clones.

`Context` derefs to `evento::context::RwContext`, an `Arc<RwLock<Context>>` where `Context` is a `HashMap<TypeId, Box<dyn Any + Send + Sync>>`.

## Graceful shutdown

```rust
let sub = builder.start(&executor).await?;
// … running …
sub.shutdown().await?;            // sends signal, awaits join handle
```

`shutdown` sends on a `oneshot` channel; the task checks it between events and again between polls, then breaks the loop. The `JoinError` returned is propagated as `tokio::task::JoinError` (panicked or cancelled).

If `accept_failure` is *off* and a handler errors past the retry budget, the task exits silently — `sub.shutdown().await?` still returns `Ok(())` because the join handle completes normally. Use tracing to surface those failures.

## Cursor recovery

Subscription state lives in the `subscriber` table (SQL) or `subscribers` partition (Fjall):

```
{ key, worker_id, cursor, lag, enabled, created_at, updated_at }
```

`SubscriptionBuilder::start` calls `executor.upsert_subscriber(key, new_worker_id)` — meaning **only one worker per `key` can be running**. If a second worker calls `start` with the same key, the previous worker's next `is_subscriber_running` poll returns `false` and that task exits. Use distinct keys (or distinct routing keys, which prefix the key) for parallel workers.

To pause a subscription externally, set `enabled = false` on its row — the worker stops on the next iteration.

## Common patterns

**Read-model writer.** Subscription with `.routing_key(...)` per partition, handlers that upsert into a SQL table keyed by `event.aggregator_id`. Combine `evento::aggregator(...).original_version(view.version)` for cross-aggregate event emission.

**Side-effects (email/webhook).** Subscription with `.retry(n).accept_failure()` so a flaky external API doesn't kill the worker. Make handlers idempotent by keying on `event.id` (a ULID).

**One-shot catch-up.** `SubscriptionBuilder::new(key).handler(…).execute(&executor)` processes everything from the cursor to head and returns — useful for migrations / backfills.

**Audit log.** `#[evento::subscription_all]` recording `event.name`, `event.aggregator_type`, `event.aggregator_id`, `event.metadata.requested_by()` into a write-only table.
