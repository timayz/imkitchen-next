---
name: sea-query
description: Reference for SeaQuery (Rust) `sea-query` crate at version 1.0.0-rc.34 — use when writing, editing, or debugging Rust code that depends on `sea-query` to build SQL for MySQL, PostgreSQL, or SQLite. Covers the `Iden` / `IdenStatic` / `DynIden` identifier system and `#[derive(Iden)]` / `#[enum_def]`, the unified `Expr` / `ExprTrait` (no more `SimpleExpr` split), `Cond` with `any!` / `all!`, `Query::select` / `insert` / `update` / `delete`, joins, ordering, `LIMIT` / `OFFSET`, locks, `OnConflict` upserts, `Func` (sum/min/max/count/cast/coalesce/if_null/...), `Table::create` / `alter` / `drop` / `rename` / `truncate`, `ColumnDef` builders, `ForeignKey`, `Index`, the `raw_query!` macro with named/array/tuple expansion, and the `sea-query-sqlx` (`SqlxBinder::build_sqlx`) integration. Also flags 1.0 breaking changes from 0.x (`SimpleExpr` is now an alias of `Expr`, `DynIden` is now `Cow<'static, str>`, `TableRef` / `ColumnRef` collapsed, `use sea_query::ExprTrait` now required) and 1.0.0-rc.34 specifics (PostgreSQL unsigned type widening, MySQL `cond_where` on indexes now emits the filter, `jiff::Zoned` removed, `PgFunc::json_table` API rewrite).
disable-model-invocation: true
---

# SeaQuery (sea-query) — 1.0.0-rc.34

SeaQuery is a dynamic SQL builder for MySQL, PostgreSQL and SQLite. Queries and schema are constructed as an AST and rendered per-backend with `MysqlQueryBuilder`, `PostgresQueryBuilder`, or `SqliteQueryBuilder`.

Source: <https://github.com/SeaQL/sea-query/tree/1.0.0-rc.34> · Docs: <https://docs.rs/sea-query/1.0.0-rc.34>

## Install

```toml
[dependencies]
sea-query = "=1.0.0-rc.34"
# Optional sqlx integration (separate crate):
sea-query-sqlx = { version = "=0.8.0-rc.16", features = ["sqlx-sqlite", "runtime-tokio-rustls"] }
```

Pin with `=` while on a release candidate — `rc` versions are not semver-compatible with each other.

### Feature flags (`sea-query`)

| Flag | Purpose |
|------|---------|
| `derive` | Enable `#[derive(Iden)]`, `#[derive(IdenStatic)]`, `#[enum_def]`, `raw_query!`, `raw_sql!` (on by default) |
| `attr` | `#[enum_def]` without the full derive surface |
| `backend-mysql` / `backend-postgres` / `backend-sqlite` | Per-backend SQL builders (all on by default) |
| `audit` | Query auditing helpers (on by default) |
| `thread-safe` | Required by `sea-query-sqlx`; makes `DynIden` `Send + Sync` |
| `hashable-value` | Implement `Hash` on `Value` (pulls in `ordered-float`) |
| `serde` | `Serialize` / `Deserialize` for AST nodes and `Value` |
| `with-chrono` / `with-time` / `with-jiff` / `with-uuid` / `with-json` / `with-rust_decimal` / `with-bigdecimal` / `with-ipnetwork` / `with-mac_address` | Map Rust types to `Value` variants |
| `postgres-array` / `postgres-interval` / `postgres-vector` / `postgres-range` | Postgres-specific value types |

### Feature flags (`sea-query-sqlx`)

| Flag | Purpose |
|------|---------|
| `sqlx-mysql` / `sqlx-postgres` / `sqlx-sqlite` / `sqlx-any` | Pick the sqlx backend (mirror `sea-query`'s `backend-*`) |
| `with-chrono` / `with-time` / `with-jiff` / `with-uuid` / `with-json` / `with-rust_decimal` / `with-bigdecimal` / `with-ipnetwork` / `with-mac_address` | Forward type support to both sqlx and sea-query |
| `runtime-tokio-rustls` / `runtime-tokio-native-tls` / `runtime-async-std-*` | Async runtime selection |
| `unimplemented-jiff-sqlx-mysql` | Opt-in acknowledgement that `with-jiff` + `sqlx-mysql` is partly unsupported |

## Iden — identifiers

`Iden` is the trait for table and column identifiers. The crate ships two derives: `Iden` (dynamic, renders via `Display`) and `IdenStatic` (static `&'static str`, cheaper).

```rust
use sea_query::Iden;

#[derive(Iden)]
enum Character {
    Table,          // -> "character"
    Id,             // -> "id"
    FontId,         // -> "font_id"
    FontSize,
}

#[derive(Iden)]
struct Glyph;       // -> "glyph"
```

Variants auto-convert to snake_case. The `Table` variant is special — it becomes the table name. Override with `#[iden = "..."]` on the enum or per variant.

`#[enum_def]` synthesizes an Iden enum from a regular struct (handy when the struct already exists for sqlx `FromRow`):

```rust
use sea_query::{Iden, enum_def};

#[enum_def]
struct Character { pub foo: u64 }
// generates: enum CharacterIden { Table, Foo }
```

### Strings vs Iden

Every place that takes an identifier accepts both an `Iden` value and a `&str` (via `IntoIden` / `Cow<'static, str>`). For one-off identifiers, plain strings are fine; for repeated use prefer an enum so renames are compile-time errors.

```rust
.from("character")               // ok
.from(Character::Table)          // preferred
.column(("font", "name"))        // (table, column) tuple — qualified
```

## Expressions — `Expr` and `ExprTrait`

In 1.0, `SimpleExpr` and `Expr` are the same type (`SimpleExpr` is a type alias for `Expr`). Operator methods live on `ExprTrait` — **you must `use sea_query::ExprTrait`** (or glob `use sea_query::*`) to call `.eq()`, `.gt()`, `.like()`, etc.

```rust
use sea_query::{Expr, ExprTrait};
```

### Constructors (`Expr::…`)

| Constructor | Result |
|-------------|--------|
| `Expr::col(c)` | Column reference (also accepts `(table, col)` tuples) |
| `Expr::column(c)` | Alias of `col` |
| `Expr::table_asterisk(t)` | `t.*` |
| `Expr::asterisk()` | `*` |
| `Expr::val(v)` / `Expr::value(v)` | Bind a Rust value as a parameter |
| `Expr::tuple([…])` | SQL row constructor `(a, b, c)` |
| `Expr::null()` | `NULL` literal |
| `Expr::current_date()` / `current_time()` / `current_timestamp()` | Standard temporal keywords |
| `Expr::keyword_default()` | `DEFAULT` keyword |
| `Expr::cust("…")` | Raw SQL fragment (no binding) |
| `Expr::cust_with_values("… $1 $2 …", [..])` | Raw SQL with positional bindings (sequence is re-numbered globally) |
| `Expr::cust_with_expr` / `cust_with_exprs` | Raw SQL splicing other `Expr`s |
| `Expr::exists(select)` / `not_exists(select)` | `EXISTS (…)` |
| `Expr::any(select)` / `some(select)` / `all(select)` | `ANY/SOME/ALL (subquery)` |
| `Expr::case(cond, then)` | Start a `CaseStatement` (chain with `.case(...)`, `.finally(...)`) |
| `Expr::custom_keyword(i)` | Inject an arbitrary keyword |

### Operators (`ExprTrait`)

| Category | Methods |
|----------|---------|
| Comparison | `eq`, `ne`, `lt`, `lte`, `gt`, `gte`, `is`, `is_not`, `equals(col)`, `not_equals(col)`, `between(a,b)`, `not_between(a,b)` |
| Null | `is_null`, `is_not_null` |
| Set | `is_in([…])`, `is_not_in([…])`, `in_tuples([…])`, `in_subquery(select)`, `not_in_subquery(select)` |
| Pattern | `like(pat)`, `not_like(pat)` (use `Expr::cust` for `ILIKE`, or the `pg_ilike` extension) |
| Logical | `and(r)`, `or(r)`, `not()`, `unary(op)`, `binary(op, r)` |
| Arithmetic | `add`, `sub`, `mul`, `div`, `modulo` |
| Bitwise | `bit_and`, `bit_or`, `left_shift`, `right_shift` |
| Aggregates (method form) | `max()`, `min()`, `sum()`, `avg()`, `count()`, `count_distinct()` |
| Misc | `if_null(v)`, `cast_as(type)`, `as_enum(type)` |

Precedence follows SQL — SeaQuery inserts the minimum parentheses needed, so generated SQL stays readable. Enable the `option-more-parentheses` feature if you want extra defensive parens.

### Aliasing in SELECTs

```rust
Query::select()
    .expr_as(Expr::col(Character::FontSize), "size")
    .from(Character::Table);
```

## Conditions — `Cond`, `any!`, `all!`

For deeply nested AND/OR trees use the `Condition` builder:

```rust
use sea_query::{Cond, Expr, ExprTrait, Query};

Query::select()
    .column("id")
    .from("glyph")
    .cond_where(
        Cond::any()
            .add(Cond::all()
                .add(Expr::col("aspect").is_null())
                .add(Expr::col("image").is_null()))
            .add(Cond::all()
                .add(Expr::col("aspect").is_in([3, 4]))
                .add(Expr::col("image").like("A%"))),
    );
```

The macro forms are equivalent and read better when leaves are simple:

```rust
use sea_query::{any, all};

q.cond_where(any![
    Expr::col(Glyph::Aspect).is_in([3, 4]),
    all![
        Expr::col(Glyph::Aspect).is_null(),
        Expr::col(Glyph::Image).like("A%"),
    ],
]);
```

Use `and_where(expr)` for plain conjunction, `and_where_option(Option<Expr>)` to skip when `None`, and `apply_if(opt, |q, v| { … })` for fluent dynamic filters.

## Statement building

Every statement has the same shape:

```rust
fn build<T: QueryBuilder>(&self, builder: T) -> (String, Values);
fn to_string<T: QueryBuilder>(&self, builder: T) -> String;
```

- **`build`** — preferred. Returns parameterized SQL + a `Values` vector you hand to the driver.
- **`to_string`** — values inlined, for tests and logging only. **Never** ship to a DB.

Schema statements (`Table::create`, `Index::create`, …) have only `build(SchemaBuilder)` and `to_string(SchemaBuilder)`.

## SELECT

```rust
use sea_query::{Expr, ExprTrait, Order, Query, PostgresQueryBuilder};

let (sql, values) = Query::select()
    .column("char_code")
    .column(("font", "name"))
    .from(Character::Table)
    .left_join(Font::Table,
        Expr::col((Character::Table, Character::FontId))
            .equals((Font::Table, Font::Id)))
    .and_where(Expr::col(Character::SizeW).is_in([3, 4]))
    .and_where(Expr::col(Character::CharCode).like("A%"))
    .order_by(Character::Id, Order::Desc)
    .limit(50)
    .offset(0)
    .build(PostgresQueryBuilder);
```

### Common chainable methods

| Method | Notes |
|--------|-------|
| `column(c)`, `columns([…])`, `expr(e)`, `expr_as(e, alias)` | Selected columns |
| `distinct()`, `distinct_on([…])` | DISTINCT / DISTINCT ON (PG only) |
| `from(t)`, `from_as(t, alias)`, `from_subquery(q, alias)`, `from_function(f, alias)`, `from_values([…], alias)` | FROM sources |
| `inner_join`, `left_join`, `right_join`, `full_outer_join`, `cross_join`, `straight_join`, `join(JoinType, …)`, `join_as`, `join_subquery`, `join_lateral` | Joins |
| `and_where`, `and_where_option`, `cond_where` | WHERE |
| `group_by_col`, `group_by_columns`, `add_group_by` | GROUP BY |
| `and_having`, `cond_having` | HAVING |
| `order_by`, `order_by_expr`, `order_by_columns`, `order_by_with_nulls` | ORDER BY |
| `limit`, `offset`, `reset_limit`, `reset_offset` | Pagination |
| `lock`, `lock_shared`, `lock_exclusive`, `lock_with_tables`, `lock_with_behavior` | `FOR UPDATE` etc. |
| `union(UnionType::Distinct/All, q)`, `unions([…])` | UNION |
| `with_cte(WithClause)` / `.with(clause)` | CTEs |
| `apply_if(opt, \|q, v\| { … })`, `apply(\|q\| { … })`, `conditions(bool, true_fn, false_fn)` | Conditional fluent steps |
| `take()` | Move the statement out of `&mut Self` |

## INSERT

```rust
let (sql, values) = Query::insert()
    .into_table(Glyph::Table)
    .columns([Glyph::Aspect, Glyph::Image])
    .values_panic([5.15.into(), "12A".into()])
    .values_panic([4.21.into(), "123".into()])
    .build(PostgresQueryBuilder);
```

`values_panic` asserts the row width matches the column list — use `values(…)?` if the count comes from runtime data.

### Upsert / `ON CONFLICT`

```rust
use sea_query::OnConflict;

Query::insert()
    .into_table(Glyph::Table)
    .columns([Glyph::Aspect, Glyph::Image])
    .values_panic([2.into(), "B".into()])
    .on_conflict(
        OnConflict::column(Glyph::Id)
            .update_columns([Glyph::Aspect, Glyph::Image])
            .to_owned(),
    );
```

`OnConflict` builders:

- `OnConflict::column(c)` / `columns([…])` — single or composite conflict target
- `OnConflict::constraint("name")` — target a named constraint (PG)
- `.do_nothing()` / `.do_nothing_on([…])` — `DO NOTHING`
- `.update_column(c)` / `.update_columns([…])` — copy from excluded row
- `.value(col, expr)` / `.values([(col, expr), …])` — explicit update expression
- `.target_cond_where(cond)` / `.target_and_where(expr)` — partial-index target predicate (PG)
- `.action_cond_where(cond)` / `.action_and_where(expr)` — `WHERE` on the update action

### `RETURNING`

```rust
use sea_query::Query;

Query::insert()
    .into_table(Glyph::Table)
    .columns([Glyph::Aspect])
    .values_panic([2.into()])
    .returning_col(Glyph::Id);
// or .returning(Query::returning().columns([Glyph::Id, Glyph::Aspect]))
```

Supported on PostgreSQL and SQLite. MySQL ignores RETURNING.

## UPDATE

```rust
Query::update()
    .table(Glyph::Table)
    .values([
        (Glyph::Aspect, Expr::value(1.23)),
        (Glyph::Image, Expr::value("123")),
    ])
    .and_where(Expr::col(Glyph::Id).eq(1));
```

Tip: each value side accepts anything implementing `Into<Expr>`, so `1.23.into()` and `"123".into()` are fine too.

## DELETE

```rust
Query::delete()
    .from_table(Glyph::Table)
    .cond_where(
        Cond::any()
            .add(Expr::col(Glyph::Id).lt(1))
            .add(Expr::col(Glyph::Id).gt(10)),
    );
```

## Functions (`Func`)

```rust
use sea_query::Func;
```

| Builder | SQL |
|---------|-----|
| `Func::sum(e)`, `min(e)`, `max(e)`, `avg(e)`, `abs(e)` | aggregates |
| `Func::count(e)`, `count_distinct(e)` | `COUNT`, `COUNT(DISTINCT …)` |
| `Func::greatest([…])`, `least([…])` | `GREATEST` / `LEAST` |
| `Func::coalesce([…])`, `if_null(a, b)` | `COALESCE`, `IFNULL/ISNULL` |
| `Func::cast_as(e, "TEXT")`, `cast_as_quoted(e, ident)` | `CAST(… AS …)` |
| `Func::lower(e)`, `upper(e)`, `char_length(e)`, `md5(e)`, `random()` | string / hash |
| `Func::bit_and(e)`, `bit_or(e)`, `round(e)`, `round_with_precision(a,b)` | numeric |
| `Func::cust(iden)` then `.arg(x)` / `.args([…])` | call any SQL function by name |

All return `FunctionCall`; `.filter(cond)` adds a `FILTER (WHERE …)` clause (PostgreSQL).

## Schema — `Table`, `ColumnDef`, `ForeignKey`, `Index`

### Create

```rust
use sea_query::{ColumnDef, ForeignKey, ForeignKeyAction, Table};

let stmt = Table::create()
    .table(Character::Table)
    .if_not_exists()
    .col(ColumnDef::new(Character::Id).integer().not_null().auto_increment().primary_key())
    .col(ColumnDef::new(Character::FontSize).integer().not_null())
    .col(ColumnDef::new(Character::Character).string().not_null())
    .col(ColumnDef::new(Character::FontId).integer().default(1))
    .foreign_key(
        ForeignKey::create()
            .name("character_fk")
            .from(Character::Table, Character::FontId)
            .to(Font::Table, Font::Id)
            .on_delete(ForeignKeyAction::Cascade)
            .on_update(ForeignKeyAction::Cascade),
    )
    .to_owned();

let sql = stmt.to_string(SqliteQueryBuilder);
```

### `ColumnDef` type builders

Common typed builders (chain after `ColumnDef::new(name)`):

- Integers: `tiny_integer`, `small_integer`, `integer`, `big_integer` and `tiny_unsigned`, `small_unsigned`, `unsigned`, `big_unsigned`
- Floats: `float`, `double`, `decimal`, `decimal_len(p, s)`, `money`, `money_len(p, s)`
- Text: `char`, `char_len(n)`, `string`, `string_len(n)`, `text`
- Binary: `binary`, `binary_len(n)`, `var_binary(n)`, `bit(len)`, `varbit(len)`, `blob`
- Temporal: `date`, `time`, `date_time`, `timestamp`, `timestamp_with_time_zone`, `year`, `interval(fields, precision)`
- Misc: `boolean`, `json`, `json_binary`, `uuid`, `cidr`, `inet`, `mac_address`, `ltree`, `vector(size)`
- Enum: `enumeration("name", [variants…])`
- Array: `array(elem_type)`
- Raw: `custom("MY_TYPE")`

Modifiers: `not_null`, `null`, `default(value_or_expr)`, `unique_key`, `primary_key`, `auto_increment`, `check(expr)`, `generated(expr, stored)`, `extra("…")`, `using(value)`, `comment("…")`.

### `Table::alter` / `drop` / `rename` / `truncate`

```rust
Table::alter().table(Font::Table).add_column(ColumnDef::new("new_col").integer().not_null().default(100));
Table::drop().table(Glyph::Table).table(Char::Table);
Table::rename().table(Font::Table, "font_new");
Table::truncate().table(Font::Table);   // not supported by SQLite — guard by backend
```

`Table::alter` also offers `drop_column`, `modify_column`, `rename_column`, `add_foreign_key`, `drop_foreign_key`. SQLite's limited `ALTER` surface means many alter operations are emitted as a no-op or rejected — branch on backend if you need them on multiple engines.

### Foreign keys (standalone)

```rust
ForeignKey::create()
    .name("FK_character_font")
    .from(Char::Table, Char::FontId)
    .to(Font::Table, Font::Id)
    .on_delete(ForeignKeyAction::Cascade)
    .on_update(ForeignKeyAction::Cascade);

ForeignKey::drop()
    .name("FK_character_font")
    .table(Char::Table);
```

SQLite cannot ADD/DROP foreign keys on an existing table — emit FKs only inside `Table::create`, or branch by backend.

### Indexes

```rust
Index::create()
    .name("idx-glyph-aspect")
    .table(Glyph::Table)
    .col(Glyph::Aspect);

Index::drop()
    .name("idx-glyph-aspect")
    .table(Glyph::Table);
```

`Index::create()` supports `.unique()`, `.if_not_exists()`, `.col((col, IndexOrder::Asc))`, `.index_type(IndexType::BTree | Hash | …)`, and `.cond_where(cond)` for partial indexes (PostgreSQL/SQLite only — see rc.34 note below for MySQL).

## `raw_query!` macro

A 1.0 addition that brings ergonomic interpolation while keeping parameter binding intact. Named scopes, nested field access, array expansion (`{..vec}`) and tuple expansion (`{..(x.0:1),}`) all sequence into proper placeholders.

```rust
let (a, b, c) = (1, 2, "A");
let d = vec![3, 4, 5];
let q = sea_query::raw_query!(
    PostgresQueryBuilder,
    r#"SELECT ("size_w" + {a}) * {b} FROM "glyph" WHERE "image" LIKE {c} AND "id" IN ({..d})"#
);
// q.sql = SELECT ("size_w" + $1) * $2 FROM "glyph" WHERE "image" LIKE $3 AND "id" IN ($4, $5, $6)
// q.values = Values(vec![1.into(), 2.into(), "A".into(), 3.into(), 4.into(), 5.into()])
```

`q` is a `RawQuery` you can pass to `sqlx::query_with(&q.sql, q.values)` (with the sqlx integration crate) or feed to your own driver glue.

The cousin `raw_sql!` macro skips binding and inlines values — only for migrations / DDL where bindings aren't needed.

## sqlx integration — `sea-query-sqlx`

`sea-query-sqlx` (workspace name; supersedes the old `sea-query-binder`) adds a `SqlxBinder` trait to every statement type. Call `.build_sqlx(builder)` to get `(String, SqlxValues)` ready to hand to sqlx.

```rust
use sea_query::{Expr, ExprTrait, Order, Query, SqliteQueryBuilder};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Row, SqlitePool};

let (sql, values) = Query::select()
    .columns([Character::Id, Character::Character, Character::FontSize])
    .from(Character::Table)
    .order_by(Character::Id, Order::Desc)
    .limit(10)
    .build_sqlx(SqliteQueryBuilder);

let rows = sqlx::query_with(&sql, values).fetch_all(&pool).await?;
// Or with FromRow:
let rows = sqlx::query_as_with::<_, MyRow, _>(&sql, values.clone()).fetch_all(&pool).await?;
```

`SqlxValues` is `Clone` — clone it if you want to reuse the same parameters for a follow-up query (e.g. `fetch_all` then `count`). The struct serializes each `Value` to the appropriate sqlx encoder; type-feature flags must match between `sea-query` and `sea-query-sqlx` (turn on `with-uuid` in both, or neither).

### Async runtime

Pick exactly one runtime feature, mirroring sqlx:

```toml
sea-query-sqlx = { version = "=0.8.0-rc.16", features = [
    "sqlx-sqlite",
    "with-chrono",
    "with-uuid",
    "with-json",
    "runtime-tokio-rustls",
] }
```

## 1.0 breaking changes vs 0.x

When porting code from `sea-query` 0.32.x:

- **`SimpleExpr` is now a type alias for `Expr`.** Drop redundant `.into()` calls and any `SimpleExpr::…` matches. Imports of `SimpleExpr` still compile, but new code should use `Expr`.
- **`use sea_query::ExprTrait` is required** to access `.eq()`, `.like()`, etc. — they moved off `Expr` itself onto the trait so the same operators apply to all expression types. A glob `use sea_query::*` brings it in.
- **`DynIden` changed shape** — it is no longer `SeaRc<dyn Iden>`; it is a struct wrapping `Cow<'static, str>`. Identifier rendering is now eager rather than lazy. If you stored `DynIden`s in maps keyed by `Arc<dyn Iden>`, switch to comparing the string form.
- **`TableRef` / `ColumnRef` enums were collapsed.** Many old variants merged into a single `Table` variant with optional schema / alias. Match arms on these enums need updating (the AST enums are now `#[non_exhaustive]`, so add a `_ => …` wildcard).
- **`Iden::unquoted` returns `&str`** instead of writing into a `fmt::Formatter`. Custom `Iden` impls written before 1.0 will not compile until updated.
- **`ConditionExpression` is no longer public.** Convert between `Condition` and `Expr` via `From`/`Into`.

## 1.0.0-rc.34 specifics

- **PostgreSQL unsigned-integer columns widen.** `SmallUnsigned` now emits `integer` (was `smallint`); `Unsigned` now emits `bigint` (was `integer`). Newly generated PG migrations for these column types differ from 0.x output.
- **MySQL partial-index `cond_where` is no longer silently dropped.** It now renders the `WHERE` clause; MySQL itself will reject it (MySQL doesn't support partial indexes). If you want a partial index on PG/SQLite only, branch by backend before calling `.cond_where(...)` for MySQL.
- **`jiff::Zoned` is gone from the `with-jiff` value API.** `Value::JiffZoned`, `ArrayType::JiffZoned`, etc. were removed because the sqlx Jiff binder cannot lossless-round-trip `Zoned`. Use one of `jiff::civil::Date` / `Time` / `DateTime` / `jiff::Timestamp`, or store zoned values as text yourself.
- **Jiff datetime column mapping shifted.** `jiff::civil::DateTime` now maps to `Timestamp` (was `DateTime`); `jiff::Timestamp` now maps to `TimestampWithTimeZone` (was `Timestamp`).
- **`PgFunc::json_table` API was rewritten.** Old chained sub-builders (`column(...).path(...).build_column()`, `nested(...).column(...).build_nested()`) are gone. Use the value-object form:

  ```rust
  use sea_query::extension::postgres::func::json_table::{Column, ExistsColumn, NestedPath};

  PgFunc::json_table(json_expr, "$.items[*]")
      .path_name("data")
      .column(Column::new("id", ColumnType::Integer).path("$.id"))
      .exists(ExistsColumn::new("has_x", ColumnType::Boolean).path("$.x"))
      .nested(NestedPath::new("$.kids[*]").column(Column::new("kid_id", ColumnType::Integer).path("$.id")));
  ```

  `explicit_path` was removed; nested paths now always render as `NESTED PATH ...`.
- **New: table partitioning** for PostgreSQL (`PARTITION BY RANGE/LIST/HASH`, `PARTITION OF`, `FOR VALUES …`) and MySQL (`PARTITION BY RANGE/LIST/HASH/KEY`, `VALUES IN`, `VALUES LESS THAN`) via `TableCreateStatement::add_partition`.
- **New: SQLx and rusqlite binders for Jiff values** (everything except `Zoned`). On MySQL the binder errors at compile time unless the `unimplemented-jiff-sqlx-mysql` opt-in feature is enabled.
- **PostgreSQL `ON CONFLICT` index expressions are now wrapped in parens** where the grammar requires it (e.g. `ON CONFLICT ("name", ("variant" IS NULL))`).

## Anti-patterns to flag

- **Calling `.eq` / `.like` / etc. without `use sea_query::ExprTrait`** — produces "no method named …" errors. Glob-import or add the trait import.
- **Building SQL via `.to_string(builder)` and shipping it to the DB.** That inlines values without escaping into the wire protocol — risk of SQL injection and double-escaping. Always use `.build(builder)` (or `.build_sqlx(builder)`) for execution; reserve `to_string` for tests and logs.
- **Mixing `=` and `>=` pins on `sea-query` and `sea-query-sqlx` during `rc.x`** — these crates ship in lock-step; an `rc.34` builder type won't match an `rc.33` `SqlxBinder`. Pin both with `=`.
- **`with-jiff` feature mismatch between `sea-query` and `sea-query-sqlx`** — produces `Value` variants the binder can't handle. Enable type features in both crates simultaneously.
- **Calling `.cond_where(...)` on `IndexCreateStatement` for MySQL.** Since rc.34 this emits the `WHERE` clause and MySQL rejects it. Branch by backend.
- **Relying on `jiff::Zoned` in `Value`** — removed in rc.34. Migrate to a non-`Zoned` Jiff type or text storage.
- **Storing `DynIden`s across threads expecting `Arc<dyn Iden>` semantics** — the type changed in 1.0 to a `Cow<'static, str>` struct. Switch to comparing by string.
- **`Table::truncate` on SQLite** — silently no-ops or errors depending on context. Use `Query::delete().from_table(t)` instead, or branch by backend.
- **`ForeignKey::create` / `ForeignKey::drop` against an existing SQLite table** — unsupported by SQLite. Define FKs inline in `Table::create` only.
- **Forgetting `to_owned()` after building.** Most builder methods return `&mut Self`; if you need an owned statement (to store, pass around, or reuse) call `.to_owned()` or `.take()` at the end of the chain.
- **Using `Expr::cust("WHERE …")` to splice user-controlled strings.** `cust` is raw SQL with no binding — only use it for SQL fragments you control. For user values use `cust_with_values("…", [vals])` or the typed `Expr` ops.
