# Creating Askama Templates

Source: <https://askama.rs/en/stable/creating_templates.html>

A template is a Rust struct (or enum) decorated with `#[derive(Template)]` and a `#[template(...)]` attribute. Struct fields become the template's variables; Askama generates the rendering code at compile time.

## Minimal example

```rust
use askama::Template;

#[derive(Template)]
#[template(path = "hello.html")]
struct HelloTemplate<'a> {
    name: &'a str,
}

let s = HelloTemplate { name: "World" }.render()?;
```

Template files live in `templates/` at the crate root by default.

## `#[template(...)]` attributes

| Attribute | Purpose |
|-----------|---------|
| `path = "file.html"` | File under `templates/`. Extension drives auto-escape and MIME. Mutually exclusive with `source`. |
| `source = "…"` | Inline template body. Requires `ext`. Mutually exclusive with `path`. |
| `ext = "html"` | File extension for `source`; controls escape mode and content type. |
| `escape = "html"` \| `"none"` \| … | Override the extension-derived escaper. |
| `print = "none"` \| `"ast"` \| `"code"` \| `"all"` | Compile-time debug output of parsed AST or generated code. |
| `block = "name"` | Render a single named block; only that block's variables are required on the struct. |
| `blocks = ["a", "b"]` | Generate sub-template accessors (`my.as_a()`, `my.as_b()`). |
| `in_doc = true` | Read template from a fenced ```` ```askama ```` block in the struct's doc comment (needs `code-in-doc` feature). Combine with `ext`. |
| `syntax = "custom"` | Use a custom-named syntax defined in the config file. |
| `config = "path.toml"` | Config file path relative to crate root. |
| `whitespace = "suppress"` \| … | Default whitespace handling. |
| `askama = $crate::__askama` | Override the path to the `askama` crate (for re-exports in libraries/macros). |

## Field → variable mapping

```rust
#[derive(Template)]
#[template(source = "{{ name }} is {{ age }}", ext = "txt")]
struct Person<'a> {
    name: &'a str,
    age: u32,
}
```

Lifetimes and generics on the struct are preserved. Visibility (`pub`, `pub(crate)`) does not affect template access; the template sees all fields.

## Enums as templates

Each variant can share one template or get its own:

```rust
#[derive(Template)]
#[template(path = "area.txt")]
enum Area {
    Square(f32),
    Rectangle { a: f32, b: f32 },
    Circle { radius: f32 },
}
```

In `area.txt`, dispatch with `{% match self %}` / `{% when … %}`.

Per-variant templates:

```rust
#[derive(Template)]
#[template(ext = "txt")]
enum AreaPerVariant {
    #[template(source = "{{ self.0 }}^2")]
    Square(f32),
    #[template(source = "{{ a }} * {{ b }}")]
    Rectangle { a: f32, b: f32 },
}
```

Variants inherit `config`, `escape`, `ext`, `syntax`, and `whitespace` from the enum's attribute, but not `block` or `print`.

Alternatively, use `block = "…"` on each variant to point at named blocks in a shared file.

## `in_doc` templates

```rust
/// ```askama
/// <div>{{ content }}</div>
/// ```
#[derive(Template)]
#[template(ext = "html", in_doc = true)]
struct Example<'a> {
    content: &'a str,
}
```

`jinja` / `jinja2` are accepted in place of `askama` for editor syntax highlighting.

## Rendering

```rust
let out: String = template.render()?;
```

`.render()` returns `askama::Result<String>`. Web-framework integrations (Actix, Axum, Rocket, Warp) wrap this and set the response's `Content-Type` from the template extension.

## Choosing `path` vs `source`

- `path` — the normal case. Keeps templates in `templates/`, where editors highlight them and they're easy to diff.
- `source` — short/embedded snippets, tests, or programmatic templates. Requires `ext`.

## Debugging

Set `print = "code"` to see the generated rendering code at compile time — useful when the template compiles but produces unexpected output.
