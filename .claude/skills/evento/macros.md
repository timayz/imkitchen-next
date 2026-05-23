# Evento macros reference

All macros live in `evento-macro` and are re-exported under `evento::` when the default `macro` feature is enabled.

## `#[evento::aggregator]` — attribute on an enum

```rust
#[evento::aggregator]
pub enum BankAccount {
    /// doc comments and field attrs are preserved
    AccountOpened { owner_id: String, initial_balance: i64 },
    MoneyWithdrawn(i64, String),         // tuple variant → tuple struct
    AccountClosed,                       // unit variant → unit struct
}
```

Expands roughly to:

```rust
#[derive(Debug, Clone, PartialEq, Default, bitcode::Encode, bitcode::Decode)]
pub struct AccountOpened {
    pub owner_id: String,
    pub initial_balance: i64,
}
impl evento::Aggregator for AccountOpened {
    fn aggregator_type() -> &'static str { /* "{CARGO_PKG_NAME}/BankAccount" */ }
}
impl evento::AggregatorEvent for AccountOpened {
    fn event_name() -> &'static str { "AccountOpened" }
}

#[derive(Debug, Clone, PartialEq, Default, bitcode::Encode, bitcode::Decode)]
pub struct MoneyWithdrawn(pub i64, pub String);
// + Aggregator + AggregatorEvent impls

#[derive(Debug, Clone, PartialEq, Default, bitcode::Encode, bitcode::Decode)]
pub struct AccountClosed;
// + Aggregator + AggregatorEvent impls

#[derive(Default)]
pub struct BankAccount;                 // the aggregator "handle"
impl evento::Aggregator for BankAccount {
    fn aggregator_type() -> &'static str { /* same string */ }
}
```

Pass extra derives as macro args — they are appended to the mandatory list:

```rust
#[evento::aggregator(serde::Serialize, serde::Deserialize)]
pub enum Order { Placed { sku: String, qty: u32 } }
```

Key facts:

- The aggregator type is computed via `format!("{}/{}", env!("CARGO_PKG_NAME"), enum_name)` inside a `LazyLock<String>`. Changing the crate name changes the persisted type.
- The mandatory derives are not configurable. If you need a different `Default` body, implement it manually outside the macro (e.g., on a unit variant after generating non-default fields by hand).
- All fields on named-variant structs are emitted `pub`. Tuple fields are emitted `pub` per position.

## `#[evento::projection]` — attribute on a struct

Adds a `cursor: String` field and implements `ProjectionCursor`. Existing derives are preserved; `Default` and `Clone` are added if missing.

```rust
#[evento::projection]
#[derive(Debug)]
pub struct AccountView {
    pub balance: i64,
    pub owner: String,
}
```

Expands to:

```rust
#[derive(Default, Clone, Debug)]
pub struct AccountView {
    pub balance: i64,
    pub owner: String,
    pub cursor: String,
}

impl evento::ProjectionCursor for AccountView {
    fn set_cursor(&mut self, v: &evento::cursor::Value) { self.cursor = v.to_string(); }
    fn get_cursor(&self) -> evento::cursor::Value { self.cursor.to_owned().into() }
}
```

Only named-field structs are supported (no tuple, no unit, no enum, no union). Extra derives can be supplied via attribute args:

```rust
#[evento::projection(serde::Serialize)]
pub struct AccountView { /* … */ }
```

**Manually implement `ProjectionCursor` instead** when you want the cursor stored as `evento::cursor::Value` directly (richer type) — the `bank` example's `AccountDetailsView` does exactly that, with `pub cursor: cursor::Value` and a hand-written impl. The attribute macro always uses `String`.

## `#[evento::handler]` — projection handler

Required signature:

```rust
async fn name(event: Event<EventType>, projection: &mut ProjectionType)
    -> anyhow::Result<()>
```

Expands to (sketch — `name` becomes both a zero-sized struct and a constructor fn returning that struct):

```rust
pub struct NameHandler;                              // PascalCase + "Handler"
fn name() -> NameHandler { NameHandler }             // constructor

impl NameHandler {
    async fn name(event: Event<EventType>, projection: &mut ProjectionType)
        -> anyhow::Result<()> { /* original body */ }
}

impl evento::projection::Handler<ProjectionType> for NameHandler {
    fn handle<'a>(&'a self, projection: &'a mut ProjectionType, event: &'a evento::Event)
        -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>>
    { Box::pin(async move {
        let event: Event<EventType> = event.try_into()?;
        Self::name(event, projection).await
    }) }

    fn event_name(&self) -> &'static str { EventType::event_name() }
    fn aggregator_type(&self) -> &'static str { EventType::aggregator_type() }
}
```

Implications:

- Register with `Projection::new(...).handler(name())` — note the `()` invocation.
- Function name is converted from `snake_case` to PascalCase for the struct name. A function named `on_account_opened` produces `OnAccountOpenedHandler`.
- Macro errors are spelled `expected first parameter: event: Event<T>` / `expected second parameter: action: Action<'_, P, E>` — they're misleading; the *actual* contract is `(Event<E>, &mut P)`. The compiler error you'll likely see is "missing trait impl" or a deref error if the second arg isn't a `&mut` reference.

Use `#[evento::debug_handler]` to dump the expanded code to `target/evento_debug_handler_macro.rs`.

## `#[evento::subscription]` — subscription handler for one event

Required signature:

```rust
async fn name<E: Executor>(
    context: &Context<'_, E>,
    event: Event<EventType>,
) -> anyhow::Result<()>
```

Expansion (sketch):

```rust
pub struct NameHandler;
fn name() -> NameHandler { NameHandler }

impl NameHandler {
    async fn name<E: Executor>(context: &Context<'_, E>, event: Event<EventType>)
        -> anyhow::Result<()> { /* original */ }
}

impl<E: evento::Executor> evento::subscription::Handler<E> for NameHandler {
    fn handle<'a>(&'a self, context: &'a evento::subscription::Context<'a, E>, event: &'a evento::Event)
        -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>>
    { Box::pin(async move {
        let event: Event<EventType> = event.try_into()?;
        Self::name(context, event).await
    }) }

    fn event_name(&self) -> &'static str { EventType::event_name() }
    fn aggregator_type(&self) -> &'static str { EventType::aggregator_type() }
}
```

Required differences vs `#[handler]`:

- Must be generic over `E: Executor` (or a concrete executor type — but generic is the idiomatic form so the handler works in tests, sqlite, postgres, etc.).
- First arg is `&Context<'_, E>` (from `evento::subscription::Context`, **not** `evento::projection::Context`). Inside, `ctx.executor` gives a borrow on the executor; `ctx.extract::<T>()` reads `Data<T>` injected via `SubscriptionBuilder::data(...)`.

## `#[evento::subscription_all]` — subscription handler for every event of an aggregate

Required signature:

```rust
async fn name<E: Executor>(
    context: &Context<'_, E>,
    event: RawEvent<AggregatorType>,           // not Event<EventType>
) -> anyhow::Result<()>
```

Difference vs `#[subscription]`: the inner event isn't decoded; `RawEvent<A>` is just `(evento::Event, PhantomData<A>)` and derefs to `&evento::Event` — so `event.name`, `event.version`, `event.data` (raw bytes), `event.metadata`, etc. are all available.

Generated `event_name()` returns the literal `"all"`. Internally the subscription dispatcher prefers the `"{type}_all"` key over per-event handlers, so:

- If you register `subscription_all` for `BankAccount`, every account event flows through it.
- Mixing `subscription_all` + `subscription` for the same aggregator on one `SubscriptionBuilder` means `subscription_all` wins; the per-event handlers will not fire.

## `#[derive(evento::Cursor)]` — cursor for paginated SQL rows

Annotate fields with `#[cursor(Column::Variant, order)]` or `#[cursor(name, Column::Variant, order)]`. Generates a `XxxCursor` struct (or `XxxNameCursor` for named cursors) plus `evento::cursor::Cursor` and `evento::sql::Bind` impls.

```rust
#[derive(sqlx::FromRow, evento::Cursor)]
pub struct AdminView {
    #[cursor(ContactAdmin::Id, 1)]
    pub id: String,
    #[cursor(ContactAdmin::CreatedAt, 2)]
    pub created_at: u64,
}
```

Expands to (sketch):

```rust
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct AdminViewCursor {
    pub i: String,        // short name derived from "id"
    pub c: u64,           // short name derived from "created_at"
}

impl evento::cursor::Cursor for AdminView {
    type T = AdminViewCursor;
    fn serialize(&self) -> Self::T { AdminViewCursor { i: self.id.to_owned(), c: self.created_at } }
}

impl evento::sql::Bind for AdminView {
    type T = ContactAdmin;
    type I = [Self::T; 2];
    type V = [sea_query::Expr; 2];
    type Cursor = Self;
    fn columns() -> Self::I { [ContactAdmin::CreatedAt, ContactAdmin::Id] }  // descending order priority
    fn values(c: AdminViewCursor) -> Self::V { [c.c.into(), c.i.into()] }
}
```

Notes:

- Order arg = sort priority. Highest order is the *primary* sort column in cursor comparisons. Use `1` for the most-specific (usually `id`), higher numbers for coarser columns (e.g. `created_at`).
- The column path's enum (`ContactAdmin`) must be a `sea_query::Iden` your project defines.
- Named cursors (`#[cursor(cursor_name, Column::Variant, order)]`) generate a `XxxCursorNameWrapper(pub Xxx)` newtype that wraps the row and implements `FromRow` transparently — useful when one row type supports multiple cursor orderings.
- All fields tagged with the same cursor name must share the same column enum (the macro takes the enum path from the first field).
- Only structs with named fields are supported.

## `#[evento::debug_handler]`

Identical to `#[evento::handler]` but additionally writes the generated tokens to `target/evento_debug_handler_macro.rs` and `include!`s them. Use it temporarily while debugging macro errors — you can `cat` the file to see what the macro emitted.
