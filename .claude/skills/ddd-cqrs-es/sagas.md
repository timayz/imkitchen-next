---
name: sagas
description: Saga patterns for long-running, cross-aggregate workflows — choreography vs orchestration, process managers, compensating actions, timeouts, retry, dedup, idempotency. End-to-end examples implemented on top of evento subscriptions.
type: reference
---

# Sagas

A saga is a sequence of local transactions across multiple aggregates (and possibly multiple bounded contexts) coordinated so that, on partial failure, the system reaches a consistent end state through **compensating actions** rather than by rolling back a distributed transaction.

The pattern exists because:

1. Distributed transactions (2PC) don't scale and create liveness problems.
2. Aggregates are individually transactional but cross-aggregate updates can't be atomic.
3. Real business processes span hours or days — far longer than a transaction can be held open.

## The two styles

```
─── Choreography ────────────────────────────────────────────────────────
Each service listens to events and emits its own events. No central brain.

   Order: OrderPlaced ──► Inventory subscribes ──► StockReserved
                                                       │
                                                       ▼
                                       Payment subscribes ──► PaymentCaptured
                                                                    │
                                                                    ▼
                                                Shipping subscribes ──► OrderShipped


─── Orchestration ───────────────────────────────────────────────────────
A process manager owns the workflow state and issues commands to each service.

   OrderPlaced ──► OrderSaga
                      │  ── command: ReserveStock ──► Inventory
                      │  ◄── event: StockReserved ───┘
                      │  ── command: ChargeCard    ──► Payment
                      │  ◄── event: PaymentCaptured ┘
                      │  ── command: Ship          ──► Shipping
                      │  ◄── event: OrderShipped   ─┘
                      ▼
                  SagaCompleted
```

| | Choreography | Orchestration |
|-|-|-|
| Coupling | Loose (each service knows only its inputs and outputs) | Tighter (orchestrator knows the whole flow) |
| Flow visibility | Hard — flow is implicit in subscriptions | Easy — flow is one piece of code |
| Adding a step | Edit one service, subscribe to the previous step | Edit the orchestrator |
| Compensation | Each service reacts to compensation events | Orchestrator emits compensations in reverse order |
| Cycles | Easy to introduce accidentally | Harder — orchestrator is the only initiator |
| Best for | Few steps, autonomous services | Many steps, complex branching, business rules |

**Pick by step count and team boundary.** 2–3 steps inside one team: choreography. 5+ steps or crossing team boundaries: orchestration. There's no universal answer — both are valid.

## Choreography example

The classic order flow, implemented as independent subscriptions.

### Events

```rust
#[evento::aggregator]
pub enum Order {
    OrderPlaced     { customer_id: String, lines: Vec<LineItem>, total: i64 },
    StockReserved,
    StockReservationFailed { reason: String },
    PaymentCaptured { charge_id: String },
    PaymentFailed   { reason: String },
    OrderShipped    { tracking_no: String },
    OrderCancelled  { reason: String },     // compensation event
}

#[evento::aggregator]
pub enum Inventory { StockReservedForOrder { order_id: String, lines: Vec<LineItem> }, /* ... */ }

#[evento::aggregator]
pub enum Payment { ChargeRequested { order_id: String, amount: i64 }, /* ... */ }
```

### Choreography subscriptions

```rust
// Step 1: Order placed → reserve stock.
#[evento::subscription]
async fn reserve_stock_on_order_placed<E: evento::Executor>(
    ctx: &evento::subscription::Context<'_, E>,
    e: Event<OrderPlaced>,
) -> anyhow::Result<()> {
    // Idempotent on order_id — Inventory aggregate's id is hashed from order_id.
    let inventory_id = inventory::reserve_for_order(
        ctx.executor(), &e.aggregator_id, &e.data.lines
    ).await?;
    Ok(())
}

// Step 2: Stock reserved → record on Order aggregate, then trigger payment.
#[evento::subscription]
async fn record_stock_reserved<E: evento::Executor>(
    ctx: &evento::subscription::Context<'_, E>,
    e: Event<inventory::StockReservedForOrder>,
) -> anyhow::Result<()> {
    // Append to the original Order aggregate.
    let order = load_order(ctx.executor(), &e.data.order_id).await?
        .ok_or_else(|| anyhow::anyhow!("missing order"))?;
    if order.has_event::<StockReserved>() { return Ok(()); }   // idempotent

    evento::aggregator(&e.data.order_id)
        .original_version(order.version())
        .event(&StockReserved)
        .commit(ctx.executor()).await?;
    Ok(())
}

// Step 3: StockReserved on Order → start payment.
#[evento::subscription]
async fn charge_on_stock_reserved<E: evento::Executor>(
    ctx: &evento::subscription::Context<'_, E>,
    e: Event<StockReserved>,
) -> anyhow::Result<()> {
    let order = load_order(ctx.executor(), &e.aggregator_id).await?.unwrap();
    payment::request_charge(ctx.executor(), &e.aggregator_id, order.total()).await?;
    Ok(())
}

// Step 4 (compensation): Stock reservation failed → cancel order.
#[evento::subscription]
async fn cancel_order_on_stock_failure<E: evento::Executor>(
    ctx: &evento::subscription::Context<'_, E>,
    e: Event<inventory::ReservationFailed>,
) -> anyhow::Result<()> {
    let order = load_order(ctx.executor(), &e.data.order_id).await?.unwrap();
    if order.is_cancelled() { return Ok(()); }
    evento::aggregator(&e.data.order_id)
        .original_version(order.version())
        .event(&OrderCancelled { reason: e.data.reason.clone() })
        .commit(ctx.executor()).await?;
    Ok(())
}
```

What's good here: each handler is small and reasons about one transition. What's hard: trying to *answer the question "where is order O right now in the workflow?"* requires reading several aggregates and reconstructing the choreography in your head. There is no single piece of code that *is* the workflow.

## Orchestration example

Same flow, but with an explicit `OrderSaga` aggregate that owns the state machine.

### Saga aggregate

```rust
#[evento::aggregator]
pub enum OrderSaga {
    /// Saga starts when an order is placed.
    SagaStarted          { order_id: String, customer_id: String, total: i64 },
    StockReservationRequested,
    StockReserved,
    PaymentRequested,
    PaymentCaptured      { charge_id: String },
    ShipmentRequested,
    ShipmentDispatched   { tracking_no: String },

    /// Compensation path:
    StockReservationFailed,
    PaymentFailed,
    SagaCompensated      { reason: String },
    SagaCompleted,
}
```

### Process manager: a single subscription that drives the saga

```rust
// Start the saga when an Order is placed.
#[evento::subscription]
async fn start_saga<E: evento::Executor>(
    ctx: &evento::subscription::Context<'_, E>,
    e: Event<OrderPlaced>,
) -> anyhow::Result<()> {
    // Saga aggregate id is derived from order_id so it's deterministic and idempotent.
    let saga_id = saga_id_for_order(&e.aggregator_id);
    if saga_exists(ctx.executor(), &saga_id).await? { return Ok(()); }   // idempotent restart

    evento::create()
        .event(&SagaStarted {
            order_id: e.aggregator_id.clone(),
            customer_id: e.data.customer_id.clone(),
            total: e.data.total,
        })
        .commit(ctx.executor()).await?;

    // Issue the first command.
    inventory::request_reservation(ctx.executor(), &e.aggregator_id, &e.data.lines).await?;
    evento::aggregator(&saga_id)
        .original_version(1)
        .event(&StockReservationRequested)
        .commit(ctx.executor()).await?;
    Ok(())
}

// Inventory replies (success) — saga advances and issues next command.
#[evento::subscription]
async fn on_stock_reserved<E: evento::Executor>(
    ctx: &evento::subscription::Context<'_, E>,
    e: Event<inventory::StockReservedForOrder>,
) -> anyhow::Result<()> {
    let saga_id = saga_id_for_order(&e.data.order_id);
    let saga = load_saga(ctx.executor(), &saga_id).await?.unwrap();
    if !matches!(saga.state, SagaState::StockReservationRequested) { return Ok(()); }

    payment::request_charge(ctx.executor(), &e.data.order_id, saga.total).await?;
    evento::aggregator(&saga_id)
        .original_version(saga.version())
        .event(&StockReserved)
        .event(&PaymentRequested)        // atomic batch — saga always advances both
        .commit(ctx.executor()).await?;
    Ok(())
}

// Inventory replies (failure) — saga compensates and ends.
#[evento::subscription]
async fn on_stock_reservation_failed<E: evento::Executor>(
    ctx: &evento::subscription::Context<'_, E>,
    e: Event<inventory::ReservationFailed>,
) -> anyhow::Result<()> {
    let saga_id = saga_id_for_order(&e.data.order_id);
    let saga = load_saga(ctx.executor(), &saga_id).await?.unwrap();
    if saga.is_terminal() { return Ok(()); }

    // Nothing to compensate yet — just close the saga and cancel the order.
    order::cancel(ctx.executor(), &e.data.order_id, &e.data.reason).await?;
    evento::aggregator(&saga_id)
        .original_version(saga.version())
        .event(&StockReservationFailed)
        .event(&SagaCompensated { reason: e.data.reason.clone() })
        .commit(ctx.executor()).await?;
    Ok(())
}

// Payment failure — must compensate by un-reserving stock.
#[evento::subscription]
async fn on_payment_failed<E: evento::Executor>(
    ctx: &evento::subscription::Context<'_, E>,
    e: Event<payment::ChargeFailed>,
) -> anyhow::Result<()> {
    let saga_id = saga_id_for_order(&e.data.order_id);
    let saga = load_saga(ctx.executor(), &saga_id).await?.unwrap();
    if saga.is_terminal() { return Ok(()); }

    inventory::release_reservation(ctx.executor(), &e.data.order_id).await?;
    order::cancel(ctx.executor(), &e.data.order_id, "payment failed").await?;
    evento::aggregator(&saga_id)
        .original_version(saga.version())
        .event(&PaymentFailed)
        .event(&SagaCompensated { reason: "payment failed".into() })
        .commit(ctx.executor()).await?;
    Ok(())
}

// Happy path completion.
#[evento::subscription]
async fn on_shipment_dispatched<E: evento::Executor>(
    ctx: &evento::subscription::Context<'_, E>,
    e: Event<shipping::ShipmentDispatched>,
) -> anyhow::Result<()> {
    let saga_id = saga_id_for_order(&e.data.order_id);
    let saga = load_saga(ctx.executor(), &saga_id).await?.unwrap();
    if saga.is_terminal() { return Ok(()); }

    evento::aggregator(&saga_id)
        .original_version(saga.version())
        .event(&ShipmentDispatched { tracking_no: e.data.tracking_no.clone() })
        .event(&SagaCompleted)
        .commit(ctx.executor()).await?;
    Ok(())
}
```

The saga aggregate is *the workflow*. Reading its events gives you the complete timeline: when it started, what advanced, where it failed, what was compensated. Adding a step means adding two events and one subscription handler.

## Compensating actions

For every forward action that has external side effects, the saga needs a defined *inverse*:

| Forward | Compensation |
|---------|--------------|
| Reserve stock | Release reservation |
| Capture payment | Refund charge |
| Ship order | (Can't unship — issue return shipment) |
| Send confirmation email | Send cancellation email |

Properties of a good compensation:

- **Semantically inverse, not byte-inverse.** You don't "un-send" an email; you send a cancellation email. The customer is informed; that's the goal.
- **Idempotent.** Compensation may run twice (saga retries).
- **Always possible.** If a step cannot be compensated, *do not put it in the saga* — or put it at the end so nothing needs unwinding after it.

**Order compensations in the reverse of the forward sequence.** If the forward path is `A → B → C → D` and D fails, compensate `C → B → A` in that order. An orchestration saga makes this explicit; a choreography saga has to be coded carefully to do the same.

## Timeouts and stalled sagas

Subscriptions react to events — but no event means no reaction. A saga that's waiting for `PaymentCaptured` from an external payment service that *never replies* will sit forever unless something pokes it.

Strategies:

1. **Periodic sweep** — a separate subscription or scheduled job runs every N minutes, queries sagas in `WaitingFor*` states older than a threshold, and emits `Timeout` events to advance or compensate them.
2. **Scheduled wakeups** — when the saga enters a waiting state, schedule an external timer (cron, queue with delay) that fires a `CheckTimeout` command after the threshold.

`evento` itself doesn't provide a timer service. Build the sweep on top of:

```rust
// Pseudocode: scheduled job, runs every minute.
async fn sweep_stalled_sagas<E: evento::Executor>(executor: &E) -> anyhow::Result<()> {
    // Query the SagaView read model for sagas in WaitingFor* states older than 10 min.
    let stalled = list_stalled_sagas(executor, Duration::minutes(10)).await?;
    for saga in stalled {
        evento::aggregator(&saga.id)
            .original_version(saga.version)
            .event(&StepTimedOut { step: saga.current_step })
            .commit(executor).await?;
        // A subscription on StepTimedOut then runs the compensation path.
    }
    Ok(())
}
```

**The timeout event is a domain event.** Persist it; it's part of the saga's history, useful for auditing and for retrying.

## Retry policy

`evento` subscriptions retry on handler error: `.retry(N)` controls the count (default 30, exponential backoff). For sagas this is *infrastructural* retry — transient failures (network blip, deadlock) retried automatically.

Domain-level retry is different. "Payment provider says decline → retry with a different card" is *new commands from the user*, not handler retry. Don't conflate them.

Two retry layers, kept separate:

| Layer | What | Where |
|-------|------|-------|
| Infrastructural | "the executor connection dropped" | `evento` subscription `.retry(N)` |
| Domain | "the charge declined, try again with another card" | New command from the user, new event, new saga step |

If domain retries are first-class in your flow (`PaymentRetried` is a real event), they appear in the saga's history. That's good — auditable.

## Idempotency for saga handlers

A saga subscription **must** be idempotent on every handler. Recipe:

1. **Derive the saga's aggregate id deterministically** from the upstream event's identity (e.g., `saga_id_for_order(order_id) = sha3("order-saga:" + order_id)`). Re-running `SagaStarted` for the same order hits the existing saga, not a new one.
2. **Check current state before advancing**:
   ```rust
   if !matches!(saga.state, SagaState::StockReservationRequested) { return Ok(()); }
   ```
   This short-circuits replays whose effect has already been applied.
3. **Use optimistic concurrency on every commit** — `.original_version(saga.version())`. Two concurrent deliveries of the same upstream event will race; one wins, one gets `InvalidOriginalVersion`, retries, sees the state is already advanced, returns `Ok`.

## Choreography vs orchestration — when to switch

You start with choreography. It works for the first 3 steps. Then:

- Step count creeps up.
- A new step needs to know "which step are we on?" — and you can't answer without reading 4 aggregates.
- Compensation requires undoing in a specific order, and no piece of code expresses that order.
- A new product manager asks "how long does the order flow take from placed to shipped, on average?" and you realize you have to reconstruct it from logs.

These are signs to introduce a saga aggregate. It does not replace the choreography — it sits *alongside* the existing aggregates, reading their events and emitting its own state events. Each step still happens on the right aggregate; the saga *records* the flow.

You can migrate incrementally: introduce the saga aggregate, start writing its events from new orders forward, leave old orders in pure choreography. The saga aggregate is just another consumer.

## Common pitfalls

- **Saga calls aggregate methods directly inside the handler.** The saga handler should issue commands (which are validated, optimistic-concurrency-checked, and committed properly). Reaching into another aggregate's state to "just append an event" skips its invariants.
- **One huge subscription with `if/else` branches per event type.** Each transition is a separate handler. Combining them defeats `evento`'s per-event-type dispatch and makes the code unreadable.
- **No compensation defined for an external side effect.** Sending an email with no cancellation path *is* a design choice — make it consciously, and put it at the *end* of the saga so nothing after it can fail.
- **Holding a database transaction across multiple aggregates.** That's not a saga; that's reaching for 2PC. Each saga step is its own local transaction.
- **Confusing saga state with aggregate state.** The saga tracks *workflow* state ("waiting for payment"). The aggregate tracks *domain* state ("balance is 100"). Two aggregates.
- **Choreography with cycles.** A subscribes to B's event, B subscribes to A's. Easy to introduce, hard to debug. Add an orchestrator at the first sign of cyclicality.
- **Forgetting that subscription handlers must be generic over `E: Executor`.** Saga handlers commit events; they need to be generic.
- **Routing key surprises.** If you set a routing key on the upstream aggregate, your saga's subscription must use `.routing_key(...)` (or `.all()`) — otherwise the default filter (`RoutingKey::Value(None)`) silently drops the events.
- **Saga aggregate gets fat.** If it accumulates dozens of events per workflow, snapshot it (see [`event-sourcing.md`](./event-sourcing.md)). It's just another aggregate, snapshot-eligible.
- **Treating sagas as a free pass for distributed transactions.** They are not. They are *eventually* consistent and may pause in inconsistent intermediate states (`stock reserved but payment not yet captured`). Design the read models so users see a meaningful status, not a half-finished mess.
