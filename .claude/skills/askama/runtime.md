# Askama Runtime Values

Source: <https://askama.rs/en/stable/runtime.html>

Most Askama data flows through struct fields, which are resolved at compile time. The runtime-values API is the escape hatch for passing dynamic, type-erased context that isn't known when the struct is defined — typically used by filters or by templates that need a shared "environment" without bloating the template struct.

## The `Values` trait

Any type implementing `askama::Values` can be passed at render time. The standard implementations cover the common cases:

- `HashMap<&str, Box<dyn Any>>` — multi-entry bag
- `(&str, &dyn Any)` — single key/value tuple
- `&dyn Values`, `&[…]` of pairs — for forwarding

Values store anything implementing `std::any::Any`, so heterogeneous types coexist.

## Setting values from Rust

```rust
use std::any::Any;
use std::collections::HashMap;

let mut values: HashMap<&str, Box<dyn Any>> = HashMap::new();
values.insert("name", Box::new("Bibop"));
values.insert("age", Box::new(12u32));

let out = template.render_with_values(&values)?;
```

For a single value:

```rust
let v: u32 = 12;
let tuple: (&str, &dyn Any) = ("age", &v);
template.render_with_values(&tuple)?;
```

## Reading values inside a template

Two equivalent forms:

```jinja
{# filter form — flows nicely in a chain #}
{{ "name" | value::<&str> }}

{# function form #}
{{ askama::get_value::<&str>("name") }}
```

Both:

- Require the type parameter explicitly (`::<T>`).
- Return a `Result`, raising `ValueType` (wrong type) or `ValueMissing` (key absent).
- Can be combined with `assigned_or` / `defined_or` fallbacks for soft lookups.

## Reading values inside a custom filter

The second parameter of a custom filter is `&dyn askama::Values`:

```rust
mod filters {
    use askama::{Values, get_value};

    pub fn greet(s: &str, env: &dyn Values) -> askama::Result<String> {
        let greeting: &str = get_value::<&str>(env, "greeting").unwrap_or("Hello");
        Ok(format!("{greeting}, {s}!"))
    }
}
```

This is how filters reach runtime data without taking it as an explicit argument at each call site.

## When to reach for runtime values

- A filter or helper needs context (locale, current user, feature flags) and you don't want every template struct to carry it.
- A render path shares many ad-hoc parameters; bundling them into a `HashMap` is easier than threading them through structs.
- You're integrating with a request-scoped environment (web frameworks) and want one canonical "bag of values" per request.

Otherwise — prefer struct fields. They're checked at compile time, while runtime values fail at render time.
