---
name: ddd
description: Domain-Driven Design strategic (bounded contexts, context maps, ubiquitous language, ACL) and tactical (entity, value object, aggregate, domain event, repository, factory, domain service) patterns, mapped to evento constructs.
type: reference
---

# Domain-Driven Design

DDD has two halves:

- **Strategic** — how you carve the system into contexts and how those contexts talk.
- **Tactical** — how you model behavior *inside* one context.

Most teams over-invest in tactical (entities, value objects) and under-invest in strategic (where do the seams go?). Strategic decisions cost more to get wrong.

## Strategic patterns

### Ubiquitous language

A vocabulary shared by domain experts and code. The terms in your code (`BankAccount`, `MoneyDeposited`, `OverdraftRequested`) should match the terms the business uses in conversation. *In writing*, prefer the business term over a technical synonym — `subscriber` not `user`, `shipment` not `record`.

This is the cheapest DDD practice and the one with the highest leverage. It's also the only one you cannot skip: every other pattern presupposes that "the model" is agreed-upon.

**Smell**: when developers and the business use different words for the same thing (e.g. dev says "user", business says "merchant"), or the same word for different things ("order" means a cart in one team's mouth and a fulfilled shipment in another's). Each one is the seam of an undiscovered bounded context.

### Bounded context

A consistency boundary around a model. Inside a bounded context, every term has one meaning. Across contexts, the same word may mean different things — `Customer` in *Billing* (has a payment method) is not `Customer` in *Shipping* (has an address).

A bounded context owns:
- Its ubiquitous language
- Its aggregates and events
- Its database / event store (or a logical partition)
- Its deployment unit (often)

**One bounded context, one event store partition.** With `evento`, the simplest mapping is one `Executor` per context. If contexts share a database for operational reasons, use distinct `aggregator_type` namespaces (recall: `evento` derives the type as `"{cargo_pkg_name}/{Enum}"`, so giving each context its own crate is a clean separator).

### Context maps and integration patterns

How two bounded contexts relate. The standard taxonomy:

| Relationship | What it means | When to use |
|--------------|---------------|-------------|
| **Shared kernel** | Two contexts share a small piece of model code | Same team, very stable shared concepts only |
| **Customer/supplier** | Upstream context cares about downstream's needs | Cooperative teams, sequential dependency |
| **Conformist** | Downstream just accepts upstream's model as-is | Cheap; you give up modeling power |
| **Anti-corruption layer (ACL)** | Downstream translates upstream's model into its own | Expensive but isolates you from upstream churn |
| **Open host service** | Upstream publishes a stable protocol many can consume | Many consumers |
| **Published language** | Schema all parties agree on (often event schemas) | Pair with Open host |
| **Separate ways** | No integration at all | Best when integration cost > value |

**ACL in code**: a translator function at your context's edge that takes an inbound integration event and emits a *local* domain event. With `evento`, an ACL is typically a subscription on an *upstream* aggregator type that calls `create()` / `aggregator()` on a *local* aggregator:

```rust
// Upstream context publishes UserSignedUp (from `billing` crate).
// Downstream `notifications` context translates it to a local event.
#[evento::subscription]
async fn translate_user_signup<E: evento::Executor>(
    ctx: &evento::subscription::Context<'_, E>,
    e: Event<billing::UserSignedUp>,
) -> anyhow::Result<()> {
    evento::create()
        .event(&notifications::PreferenceProfileInitialized {
            external_user_id: e.data.user_id.clone(),
            channel: notifications::Channel::Email,
        })
        .commit(ctx.executor()).await?;
    Ok(())
}
```

The translator deliberately drops or renames upstream fields that don't fit *our* model. That decoupling is the whole point — when upstream renames `user_id` to `customer_uuid`, you change the ACL, not your whole context.

### Domain / sub-domain / context

These are often conflated. Quick disambiguation:

- **Domain** — the business as a whole (e.g., "e-commerce").
- **Subdomain** — a problem area within the domain. Three flavors:
  - *Core* — the part that gives you competitive edge. Invest heavily.
  - *Supporting* — necessary but not differentiating. Build minimally.
  - *Generic* — the part you could buy off the shelf. Don't custom-build.
- **Bounded context** — a *solution* boundary. Ideally one context per subdomain, but it's a design choice, not a given.

Use the strategic distinction to allocate effort. Don't apply full tactical DDD to a generic subdomain.

## Tactical patterns

### Entity

An object identified by a stable identity (an ID) rather than by its attributes. Two `Customer` entities with the same name are different if their IDs differ. `evento` aggregates are entities — their identity is the aggregate ID (a ULID by default).

```rust
let id_1 = evento::create()
    .event(&CustomerRegistered { name: "Ada".into() })
    .commit(&executor).await?;

let id_2 = evento::create()
    .event(&CustomerRegistered { name: "Ada".into() })
    .commit(&executor).await?;
// id_1 != id_2 — these are two different customers who happen to share a name.
```

### Value object

An object identified by its attributes — replace, don't mutate. `Money { amount: 100, currency: "USD" }` is equal to any other `Money` with the same values. Value objects are typically immutable structs without an ID.

In Rust + `evento`, value objects appear as fields *inside* event payloads:

```rust
#[derive(Debug, Clone, PartialEq, Default, bitcode::Encode, bitcode::Decode)]
pub struct Money { pub amount: i64, pub currency: String }

#[evento::aggregator]
pub enum Wallet {
    Funded { amount: Money },     // Money is a value object
    Spent  { amount: Money },
}
```

**Rule of thumb**: if you find yourself adding `.id` to a struct just to track it across changes, it wants to be an entity. If you find yourself comparing two of them with `==`, it wants to be a value object.

### Aggregate

A cluster of related entities and value objects treated as a single unit for the purpose of data changes. One entity in the cluster is the **aggregate root**, and all external references point only to the root.

The aggregate is the **consistency boundary**:
- Invariants are enforced within one aggregate, in one transaction.
- Across aggregates, consistency is eventual — coordinated by sagas, not transactions.

**Sizing**: keep aggregates small. The single biggest design mistake in DDD is making aggregates too large because they "feel related". A big aggregate becomes a contention point (every change blocks every other change) and a refactoring trap (its invariants ossify).

Two heuristics that pull in opposite directions, and you have to balance them:

1. *Invariants*: anything that must be true together belongs in the same aggregate.
2. *Change rate*: anything frequently changing together is fine; anything that changes at very different cadences should split.

In `evento`, the aggregate is the `enum` you annotate with `#[evento::aggregator]`. Its events all live under one `aggregator_type + aggregator_id` stream, versioned and concurrency-checked together.

```rust
#[evento::aggregator]
pub enum Order {
    OrderPlaced     { customer_id: String, lines: Vec<LineItem> },
    LineItemAdded   { sku: String, qty: u32 },
    LineItemRemoved { sku: String },
    OrderSubmitted,
    OrderCancelled  { reason: String },
}
```

Here, line items are part of the `Order` aggregate (an order without items makes no sense). But the `Customer` is referenced *by id* — `Customer` is its own aggregate elsewhere.

### Domain event

A fact about something that has happened in the domain. Past tense. Immutable. Not a command, not a notification — a record.

Good event names: `MoneyDeposited`, `OrderSubmitted`, `ShipmentDispatched`.
Bad event names: `UpdateOrder`, `OrderChanged`, `ProcessPayment`, `DepositRequest`.

Events should be **business-meaningful**, not CRUD-shaped:
- `CustomerAddressChanged { new_address }` — good if "address change" is a domain concept.
- `CustomerUpdated { …all fields… }` — bad. Reads as "someone touched the row." Loses intent. Hostile to projections and sagas.

In `evento`, the `#[evento::aggregator]` macro turns each variant into an event struct. Variants should be specific business events, not catch-all CRUD updates.

**Schema rule**: events are forever. Once persisted, you live with their shape (or with the migration burden). See [`event-sourcing.md`](./event-sourcing.md) for evolution strategies.

### Repository

The collection-like abstraction for retrieving an aggregate. In classical DDD, `repo.get(id) -> Aggregate`.

In an event-sourced design, the repository pattern is mostly *implicit*: you "get" an aggregate by replaying its events into a projection.

```rust
// "Repository load" — replay events into a projection.
pub async fn load_account<E: evento::Executor>(
    executor: &E, id: &str,
) -> anyhow::Result<Option<AccountView>> {
    evento::projection::Projection::<_, AccountView>
        ::new::<BankAccount>(id)
        .handler(on_account_opened())
        .handler(on_money_deposited())
        .handler(on_money_withdrawn())
        .execute(executor).await
}
```

If you find yourself wanting a `repository.save(aggregate)` API on top of `evento`, you usually don't — `commit(&executor)` already does that. Resist the urge to wrap `evento::create()` / `evento::aggregator()` in a repository abstraction "for consistency" unless it's pulling its weight.

### Factory

A function or service that constructs a valid aggregate. In ES, the factory's job is to validate inputs and emit the *first* event(s) of the aggregate.

```rust
pub async fn open_account<E: evento::Executor>(
    executor: &E, owner_id: String, initial_deposit: i64,
) -> anyhow::Result<String> {
    anyhow::ensure!(initial_deposit >= 0, "negative initial deposit");
    let id = evento::create()
        .event(&AccountOpened { owner_id, initial_balance: initial_deposit })
        .commit(executor).await?;
    Ok(id)
}
```

Factories are usually plain async functions in Rust — no need for a dedicated trait or struct.

### Domain service

Behavior that doesn't naturally live on one aggregate or value object. Use sparingly — most behavior should be on an aggregate. A domain service is appropriate when:

- The operation spans two aggregates (often this is actually a *saga*, see [`sagas.md`](./sagas.md)).
- The operation is computational and stateless (e.g., a pricing calculation that consults a `PriceList` value object).

**Anti-pattern**: making everything a "service" because it's the easiest place to put code. The result is anemic aggregates and a "service" layer that becomes a transaction script.

## Putting it together

A worked example for one bounded context: a *Subscriptions* context for a SaaS billing system.

```rust
// ─── Aggregate ────────────────────────────────────────────────────────
#[evento::aggregator]
pub enum Subscription {
    Started   { plan_id: String, customer_id: String, started_at: i64 },
    Renewed   { period_end: i64 },
    Cancelled { reason: CancellationReason, effective_at: i64 },
    Reactivated,
}

// ─── Value objects (inside events) ────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Default, bitcode::Encode, bitcode::Decode)]
pub enum CancellationReason { #[default] Voluntary, NonPayment, ToS }

// ─── Factory ─────────────────────────────────────────────────────────
pub async fn start_subscription<E: evento::Executor>(
    executor: &E, plan_id: String, customer_id: String,
) -> anyhow::Result<String> {
    // Domain rule: a customer cannot have two active subscriptions for the same plan.
    // (Enforced by a uniqueness projection — see cqrs.md for that pattern.)
    let started_at = chrono::Utc::now().timestamp();
    evento::create()
        .event(&Started { plan_id, customer_id, started_at })
        .commit(executor).await
        .map_err(Into::into)
}

// ─── Command handler (CQRS write side) ────────────────────────────────
pub async fn cancel_subscription<E: evento::Executor>(
    executor: &E, subscription_id: &str, reason: CancellationReason,
) -> anyhow::Result<()> {
    let view = load_subscription(executor, subscription_id).await?
        .ok_or_else(|| anyhow::anyhow!("unknown subscription"))?;
    anyhow::ensure!(view.is_active(), "already cancelled");

    evento::aggregator(subscription_id)
        .original_version(view.version())
        .event(&Cancelled { reason, effective_at: view.period_end })
        .commit(executor).await?;
    Ok(())
}
```

Each piece does one job:
- The aggregator owns invariants ("can't cancel twice").
- Value objects (`CancellationReason`) live inside events.
- Factories produce the first event.
- Command handlers load + validate + commit.

What's *not* here is what's important too: no repository class, no service layer for the sake of layering, no "manager" objects. DDD in this style is mostly named functions plus aggregates plus events.

## Common pitfalls

- **Aggregate too big**. If two unrelated changes contend on the same `original_version`, split the aggregate. Contention is the symptom of a wrong boundary.
- **Aggregate too small**. If two aggregates have an invariant that must be true together and you're "syncing them with a saga", you've moved a consistency rule out of the model — the saga can fail or lag. Merge them.
- **Cross-aggregate references with object pointers**. References are by *ID only* across aggregate boundaries. If `Order` holds a `Customer` instance instead of a `customer_id`, you're going to load (and lock, and serialize) the wrong scope.
- **CRUD-shaped events**. `OrderUpdated { …everything… }` defeats the entire purpose. Find the actual domain events: `OrderItemAdded`, `OrderShippingChanged`, `OrderDiscounted`.
- **Bounded contexts on org-chart lines**. Conway's law is real, but draw boundaries based on the language and invariants, *then* align the org if you can. Letting the org chart dictate model boundaries leads to leaky models.
- **No ACL on integration boundaries**. Every external system has a different model. If you let its terms leak into your context, every change to *their* schema becomes a change to *yours*.
- **Wrapping evento in a repository abstraction**. Possible, sometimes useful, often premature. `evento::create()` / `aggregator()` are the repository.
