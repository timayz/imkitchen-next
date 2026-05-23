---
name: askama
description: Reference for Askama (Rust) template syntax — use when writing, editing, or debugging .html/.j2/.jinja templates in Rust projects that use the `askama` crate, or when generating template strings via `#[template(source = "…")]`. Covers variables, filters, control flow, inheritance, macros, includes, whitespace control, and escaping. Companion files in this skill directory: `filters.md` (full filter catalog), `creating-templates.md` (`#[derive(Template)]` and `#[template(...)]` attributes), `runtime.md` (runtime values API).
disable-model-invocation: true
---

# Askama Template Syntax

Askama is a type-safe, compile-time template engine for Rust with Jinja-like syntax. Templates are bound to a struct via `#[derive(Template)]` and `#[template(path = "…")]`.

Source: <https://askama.rs/en/stable/template_syntax.html>

Companion references in this directory:

- [`creating-templates.md`](./creating-templates.md) — `#[derive(Template)]`, all `#[template(...)]` attributes, enums as templates.
- [`filters.md`](./filters.md) — complete built-in filter catalog and custom-filter rules.
- [`runtime.md`](./runtime.md) — runtime values, `render_with_values`, `get_value`.

## Delimiters

| Delimiter | Purpose |
|-----------|---------|
| `{{ … }}` | Expression — renders a value |
| `{% … %}` | Statement — control flow, declarations |
| `{# … #}` | Comment (nestable) |

## Variables & expressions

- `{{ name }}` — field on the template context struct
- `{{ user.name }}` — nested field access
- `{{ crate::MAX_USERS }}` — Rust constants/paths
- Operators follow Rust precedence: `+ - * / %`, comparison, `&&`, `||`, parentheses for grouping
- Bit ops use word forms to avoid filter ambiguity: `bitand`, `bitor`, `xor`
- `as` cast works for primitive types only
- String concatenation: `{{ a ~ b ~ c }}` (spaces required around `~`)

## Assignments

```jinja
{% let name = user.name %}
{% let mut counter = 0 %}
{% mut counter += 1 %}
```

Variables can shadow and be `mut`. `set` is also accepted (Jinja compatibility).

**Block form** (assign computed/rendered content):

```jinja
{% let x %}
  {{ function() }} = {{ a * b }}
{% endlet %}
```

**Deferred declaration:**

```jinja
{% decl val %}
{% if cond %}{% let val = "foo" %}{% endif %}
```

## Filters

Chain with `|`; arguments in parens:

```jinja
{{ "{:?}"|format(name|escape) }}
{{ value | safe }}
{{ value | escape }}   {# or | e #}
```

**Filter block** applies filters to a span:

```jinja
{% filter lower|capitalize %}
  {{ text }}
{% endfilter %}
```

Custom filters live in a `filters` module in the context's scope.

For the complete list of built-in filters (string, escaping, collection, formatting, fallbacks, references, `json`/`tojson`) and rules for defining custom filters, see [`filters.md`](./filters.md).

## Control flow

### `if` / `else if` / `else`

```jinja
{% if users.is_empty() %}
  No users
{% else if users.len() == 1 %}
  1 user
{% else %}
  {{ users.len() }} users
{% endif %}
```

**`if let`** for `Option`/`Result`/enum patterns:

```jinja
{% if let Some(u) = current_user %}{{ u.name }}{% endif %}
```

**Existence check:**

```jinja
{% if var is defined %}…{% endif %}
{% if var is not defined %}…{% endif %}
```

### `for`

```jinja
{% for item in items if item.active %}
  {{ loop.index }}. {{ item.name }}
{% else %}
  empty
{% endfor %}
```

Loop vars: `loop.index`, `loop.index0`, `loop.first`, `loop.last`. Body may use `{% break %}` and `{% continue %}`.

### `match`

```jinja
{% match result %}
  {% when Ok(v) %} ok: {{ v }}
  {% when Err(e) %} err: {{ e }}
{% endmatch %}
```

Patterns support literals, destructuring, alternatives, and wildcards:

```jinja
{% match n %}
  {% when 1 | 2 | 3 %} low
  {% when [first, ..] %} non-empty slice
  {% else %} other
{% endmatch %}
```

Optional `{% endwhen %}` improves linter compatibility.

## Template inheritance

**Base** (`base.html`):

```jinja
<!DOCTYPE html>
<html>
  <head><title>{% block title %}Default{% endblock %}</title></head>
  <body>{% block content %}<p>placeholder</p>{% endblock %}</body>
</html>
```

**Child:**

```jinja
{% extends "base.html" %}

{% block title %}Page{% endblock %}

{% block content %}
  <h1>Hi</h1>
  {{ super() }}
{% endblock %}
```

- `super()` renders the parent block's content.
- Content outside blocks in the child template is ignored.
- `endblock title` (named close) is allowed.

**Block fragments** — render one block independently from Rust:

```rust
#[derive(Template)]
#[template(path = "page.html", block = "content")]
struct ContentFragment { /* … */ }
```

## Includes

```jinja
{% for item in items %}
  {% include "item.html" %}
{% endfor %}
```

Path must be a string literal (resolved at compile time). Included templates see the calling context.

## Macros

```jinja
{% macro heading(title, subtitle = "default") %}
  <h1>{{ title }}</h1>
  <h2>{{ subtitle }}</h2>
{% endmacro %}

{{ heading("Title") }}
{{ heading("Title", "Sub") }}
{{ heading(title = "Title", subtitle = "Sub") }}
```

**Type annotations:**

```jinja
{% macro show(value: Option<u32>) %}
  {% if let Some(v) = value %}{{ v }}{% endif %}
{% endmacro %}
```

**Import from another file:**

```jinja
{% import "macros.html" as m %}
{{ m::heading("Title") }}
```

**Named arguments:** allowed in any order after positional args. Optional args (with defaults) come last in the definition.

**Call blocks** — pass a body to a macro:

```jinja
{% macro centered() %}<center>{{ caller() }}</center>{% endmacro %}

{% call centered() %}Hello{% endcall %}
```

**Call block with arguments:**

```jinja
{% macro list_users(users) %}
  {% for u in users %}<li>{{ caller(u) }}</li>{% endfor %}
{% endmacro %}

{% call(user) list_users(users) %}
  Name: {{ user.name }}
{% endcall %}
```

Inside a macro, guard with `{% if caller is defined %}` to support both invocation styles.

## Functions, methods, closures

| Call site | Syntax |
|-----------|--------|
| Field (function-typed) | `{{ foo(arg) }}` |
| Free function in template module | `{{ self::function(arg) }}` |
| Public path | `{{ crate::module::function(arg) }}` |
| Method on `self` | `{{ self.method(arg) }}` or `{{ method(arg) }}` |
| Trait method | `{{ Self::method(arg) }}` |
| Closure | `{{ (closure)(arg) }}` |

## Struct instantiation

```jinja
{{ MyStruct { field1: 1, field2: "v" }.method() }}
{{ MyStruct { field1: 1, ..other } }}
```

## Whitespace control

Default: whitespace preserved except trailing newline of the file.

| Marker | Effect |
|--------|--------|
| `-`    | Suppress whitespace |
| `~`    | Minimize to a single character (newline if any) |
| `+`    | Preserve explicitly |

```jinja
{% if x %}
  {{- y -}}
{% endif %}
```

Priority when markers collide: Suppress > Minimize > Preserve. Configure globally via `#[template(whitespace = "suppress")]` or `askama.toml`.

## HTML escaping

- Auto-escape is on for `.html`, `.htm`, `.xml` templates (OWASP rules: `<`, `>`, `&`, `"`, `'`).
- `{{ value | safe }}` — disable escaping for this value.
- `{{ value | escape }}` or `| e` — force escape in unescaped contexts.
- `#[template(escape = "none")]` — disable for the whole template.
- Implement `askama::filters::HtmlSafe` on a type to mark its `Display` output safe.

## Comments

```jinja
{# plain #}
{# outer {# nested #} still inside #}
```

## References, deref, `?`

```jinja
{% let x = &"value" %}
{% if *x == "value" %}…{% endif %}

{{ some_result? }}          {# unwrap or fail render #}
{% let v = other_result? %}
```

## Rust macros inside templates

```jinja
{% let text = format!("{}", 12) %}
```

Variables passed to Rust macros need explicit binding so Askama tracks them:

```jinja
{% let entity = entity %}
{{ test_macro!(entity) }}
```

## Rendering nested templates

```rust
#[derive(Template)]
#[template(source = "Section: {{ inner }}", ext = "txt")]
struct Outer { inner: Inner }
```

If `Inner` renders HTML, use `{{ inner | safe }}` or implement `HtmlSafe` on `Inner`.

For recursive structures, call `.render()` directly:

```jinja
{% for child in children %}{{ child.render()? }}{% endfor %}
```

## Working tips

- Templates are checked at compile time — a typo in a field name is a `cargo build` error, not a runtime one. When debugging, read the compiler error carefully; it points at the template path and line.
- Auto-escape interacts with `| safe`: never apply `| safe` to user-controlled data.
- `{% include %}` is compile-time; the path can't be dynamic — use `{% if %}`/`{% match %}` to choose between includes.
- Block inheritance flattens at compile time; `super()` calls are inlined.
- For exhaustive enum rendering, prefer `{% match %}` over `if let` chains so the compiler enforces coverage.
