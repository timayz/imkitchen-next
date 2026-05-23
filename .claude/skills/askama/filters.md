# Askama Built-In Filters

Source: <https://askama.rs/en/stable/filters.html>

Filters apply with `|` and chain left-to-right. Arguments go in parens: `value | filter(arg1, arg2)`.

## String

| Filter | Effect | Example |
|--------|--------|---------|
| `capitalize` | First char upper, rest lower | `"hello" \| capitalize` → `"Hello"` |
| `lower` / `lowercase` | All lowercase | `"HI" \| lower` → `"hi"` |
| `upper` / `uppercase` | All uppercase | `"hi" \| upper` → `"HI"` |
| `title` / `titlecase` | Capitalize each word | `"hello WORLD" \| title` → `"Hello World"` |
| `trim` | Strip leading/trailing whitespace | `" hi " \| trim` → `"hi"` |
| `truncate(len)` | Cap length, append `…` if cut | `"hello" \| truncate(2)` → `"he..."` |
| `center(width)` | Pad with spaces, centered | `"a" \| center(5)` → `"  a  "` |
| `indent(width, [first], [blank])` | Indent each line | `"a\nb" \| indent(4)` → `"a\n    b"` |
| `wordcount` | Count words | `"a b c" \| wordcount` → `3` |

## Escaping & safety

| Filter | Effect | Example |
|--------|--------|---------|
| `escape` / `e` | HTML-escape `<`, `>`, `&`, `"`, `'` | `"<a>" \| e` → `"&lt;a&gt;"` |
| `escape("html")` | Force a specific escaper | overrides auto-escape choice |
| `safe` | Mark already-safe; skip escaping | `"<p>" \| safe` → `<p>` |
| `urlencode` | Percent-encode reserved chars | `"a?b" \| urlencode` → `"a%3Fb"` |
| `urlencode_strict` | Like `urlencode` but also escapes `/` | |

Never apply `safe` to user-controlled input.

## HTML content shaping

| Filter | Effect |
|--------|--------|
| `linebreaks` | `\n` → `<br />`, blank lines → `<p>` wrap |
| `linebreaksbr` | every `\n` → `<br />`, no `<p>` wrap |
| `paragraphbreaks` | blank lines → `<p>` wrap; single `\n` kept verbatim |

```jinja
{{ "hello\nworld\n\nfrom\naskama" | linebreaks }}
{# → <p>hello<br />world</p><p>from<br />askama</p> #}
```

## Numeric & formatting

| Filter | Effect | Example |
|--------|--------|---------|
| `filesizeformat([precision])` | Human-readable size | `1024 \| filesizeformat` → `"1.02 KB"` |
| `format(args…)` | First arg is the format string | `"{:?}" \| format(x)` |
| `fmt("…")` | Chain-friendly variant | `x \| fmt("{:?}")` |
| `pluralize([sing="", plur="s"])` | Pick form from count | `2 \| pluralize` → `"s"` ; `1 \| pluralize("mouse","mice")` → `"mouse"` |

`format` takes the format string as the *piped* value; `fmt` takes the format string as an *argument*. Prefer `fmt` when composing in a chain.

## Collections

| Filter | Effect | Example |
|--------|--------|---------|
| `join(sep)` | Concatenate with separator | `["a","b"] \| join(", ")` → `"a, b"` |
| `unique` | Iterator without duplicates (needs `std`) | `["a","b","a"] \| unique` → `["a","b"]` |
| `reject(value\|fn)` | Drop matching items | `[1,2,3,1] \| reject(1)` → `[2,3]` |

## Fallbacks

| Filter | When fallback fires |
|--------|--------------------|
| `assigned_or(fallback)` | Value is in default state — `""`, `0`, `None`, `Err` |
| `defined_or(fallback)` | Left side is an *undefined* identifier (compile-time check) |
| `default(value, [boolean=true])` | Jinja-compat hybrid — prefer the two above |

```jinja
{{ user.name | assigned_or("anonymous") }}
{{ maybe_var | defined_or("fallback") }}
```

`defined_or`'s left side must be a bare identifier — it's resolved at compile time.

## References

| Filter | Effect |
|--------|--------|
| `ref` | `x \| ref` ≡ `&x` |
| `deref` | `x \| deref` ≡ `*x` |

Useful inside filter chains where prefix `&`/`*` would be awkward.

## Feature-gated

### `json` / `tojson` — needs the `serde_json` feature

Serializes any `Serialize` value.

```jinja
{# compact #}
<script>const data = {{ value | json | safe }};</script>

{# pretty-printed with 2-space indent #}
<pre>{{ value | json(2) }}</pre>
```

In HTML *attributes*, use `| json` directly (no `| safe`) — Askama escapes for the attribute context. Inside `<script>` blocks, add `| safe`.

## Custom filters

Define in a `filters` module in the template-context's scope, or at any path:

```rust
mod filters {
    pub fn shout(s: &str, _: &dyn askama::Values) -> askama::Result<String> {
        Ok(format!("{}!!!", s.to_uppercase()))
    }
}
```

Use: `{{ name | shout }}` or `{{ name | some_module::shout }}`.

Signature requirements:

1. First parameter — the piped value. Use `impl Display` or `&str`/`&T` as appropriate.
2. Second parameter — `env: &dyn askama::Values` (runtime environment).
3. Return — `askama::Result<T>` where `T: Display`.

Optional/named arguments use `#[askama::filter_fn]` with `#[optional(default)]` annotations.

### Marking custom output HTML-safe

Either:

- Have the return type implement `askama::filters::HtmlSafe`, or
- Wrap the value in `askama::filters::Safe<T>` (unconditionally safe) or `askama::filters::MaybeSafe` (safe only in HTML-escape contexts).

This avoids needing `| safe` at every call site.
