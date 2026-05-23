---
name: twinspark
description: Reference for the TwinSpark.js HTML enhancement library — use when writing, editing, or debugging HTML that uses `ts-*` attributes (ts-req, ts-target, ts-trigger, ts-swap, ts-action, etc.), or any project that loads `twinspark.js`. Covers every documented directive, the action mini-language and its built-in commands, all trigger events/modifiers, swap strategies including morph, request/response headers, history/IndexedDB behavior, batching, and the `twinspark.func` / `twinspark.arity` JS API for registering custom actions.
disable-model-invocation: true
---

# TwinSpark

TwinSpark is a small (~8 KB gzipped) declarative HTML enhancement library: you mark elements up with `ts-*` attributes and TwinSpark issues XHRs, swaps fragments, runs short pipelines of actions, and updates history. No attribute inheritance (except `ts-data`) — what an element does is local to that element.

Source: <https://twinspark.js.org/api/>. Script: `https://cdn.jsdelivr.net/gh/piranha/twinspark-js@main/dist/twinspark.min.js`.

## Mental model

- **Origin** — the element with `ts-req` (or `ts-action`); where data is collected from.
- **Reply** — the element returned by the server (TwinSpark expects a single root element; extras are dropped unless picked up by `ts-swap-push`).
- **Target** — the element on the page that gets replaced/augmented (defaults to the origin; overridable with `ts-target`).

Default request method: `POST` on `<form>`, `GET` everywhere else. Default trigger: `submit` on forms, `click` elsewhere. Default swap: `replace`. All defaults are overridable per element.

Outgoing requests always send `Accept: text/html+partial`. The server is expected to return an HTML fragment, not a full page.

## HTML directives

### Core

| Directive | Purpose |
|-----------|---------|
| `ts-req="<url>"` | Make an XHR; replace target with reply. |
| `ts-target="<sel>"` | Pick which element receives the reply. |
| `ts-req-selector="<sel>"` | Pick a piece of the response instead of its root. |
| `ts-swap="<strategy>"` | How to merge the reply (default `replace`). |
| `ts-swap-push="<sel-to> <= <sel-from>"` | Push extra pieces of the response elsewhere on the page. |
| `ts-trigger="<event>[modifiers]"` | Event(s) that fire the request/action. |

### Additional

| Directive | Purpose |
|-----------|---------|
| `ts-req-method="GET\|POST"` | Override default method. |
| `ts-req-strategy="first\|last\|queue"` | Concurrency policy (default `queue`). |
| `ts-req-history` / `ts-req-history="replace"` | Push (or replace) browser URL after request. |
| `ts-data="<querystring-or-json>"` | Extra params; **merged up the ancestor chain**. |
| `ts-json="<json>"` | Send body as JSON (`Content-Type: application/json`). |
| `ts-req-batch` | Coalesce concurrent requests by `method + url`. |
| `ts-action="<pipeline>"` | Run client-side actions (no request needed). |
| `ts-req-before="<pipeline>"` | Actions before request (use `prevent` to cancel; `o.req` is the XHR). |
| `ts-req-after="<pipeline>"` | Actions after request, before swap (`o.response` is the reply). |

## `ts-target` selector syntax

Accepts a CSS selector or one of the keywords/modifiers below:

- `target` — the element where `ts-target` is defined (i.e. self).
- `inherit` — walk up the DOM until another `ts-target` is found. Pair with `ts-target="target"` on a parent to point children at it.
- bare CSS selector (e.g. `#cart .count`) — search from `document.root`.
- `parent <selector>` — closest ancestor matching `<selector>`.
- `child <selector>` — first descendant matching `<selector>`.
- `sibling <selector>` — first sibling (under same parent) matching `<selector>`.

`ts-action`'s `target` command accepts the same syntax.

## `ts-req-selector`

Picks a sub-element of the response — equivalent to `document.querySelector` over the parsed reply. Returns a single element. Use the `children <selector>` modifier when you actually want multiple roots from inside the matched element.

## `ts-swap` strategies

| Value | Effect |
|-------|--------|
| `replace` *(default)* | Replace target with reply. |
| `inner` | Replace target's children with reply. |
| `prepend` | Insert reply as first child of target. |
| `append` | Insert reply as last child of target. |
| `beforebegin` | Insert reply before target. |
| `afterend` | Insert reply after target. |
| `morph` | Idiomorph-style merge: keep elements with `id` in place, only update attrs/children — preserves focus, transitions, video state. |
| `morph-all` | Like `morph` but does not skip `document.activeElement`. |
| `skip` | Discard response (useful for side-effect-only requests). |

**Settling** (non-morph, non-skip): elements with an `id` get their old `class`/`style`/`width`/`height` (configurable via `data-settle`) re-applied, then swapped to new values on the next tick — this makes CSS transitions fire. New `id` elements briefly get `ts-insert`; departing `id` elements briefly get `ts-remove` — hook transitions on these classes.

## `ts-swap-push`

For multi-spot updates from a single response. The first element in the reply goes through the normal `ts-req`/`ts-swap` flow; any additional siblings with a `ts-swap-push` attribute on them are inserted wherever their selector points.

Resolution starts at the origin (so the selector can be local), then falls back to `document`. With `ts-req-batch`, origin is `document.body`. The element's own `ts-swap` controls its insertion strategy.

Server-pushed equivalent: response header `ts-swap-push: <to-selector> <= <from-selector>`.

## `ts-trigger`

Syntax: `<event>[ modifier]...[, <event>...]`. Multiple events allowed, comma-separated.

**Modifiers:**

- `delay:<N>` — wait N ms before firing (e.g. `keyup delay:300`).
- `once` — fire only once, then detach.
- `changed` — only fire if the element's `value` (or `checked` for checkbox/radio) actually changed since last trigger. Element must have a `name`.

**Special event names** (in addition to any DOM event like `click`, `submit`, `change`, `keyup`, `mouseover`…):

| Event | Fires when… |
|-------|-------------|
| `load` | Document load, or on screen-appearance if the doc is already loaded. |
| `scroll` | Window scrolls. |
| `windowScroll` | Target element scrolls. |
| `outside` | Click occurs outside the element. |
| `remove` | Element is removed (MutationObserver). |
| `childrenChange` | Children added/removed (MutationObserver). |
| `empty` | After `childrenChange`, element has 0 children. |
| `notempty` | After `childrenChange`, element has ≥ 1 child. |
| `visible` | ≥ 1% in viewport (IntersectionObserver). |
| `invisible` | Drops below 1% in viewport. |
| `closeby` | Within `window.innerHeight / 2` of viewport. |
| `away` | Inverse of `closeby`. |

## `ts-data` and `ts-json`

**`ts-data`** — the only "magic" inherited attribute. At request time TwinSpark walks the ancestor chain collecting `ts-data` values and merges them into a `FormData`, plus the origin element's own value (`input`/`select`/`textarea`) or full form data (on `<form>`).

- Format: query string (`a=1&b=2`) **or** JSON if the first character is `{`.
- Multiple values per key are kept (FormData semantics).
- Setting a key to an empty value deletes it from the merged FormData.

**`ts-json`** — for nested data structures beyond what FormData can express.

- Value is parsed as JSON and sent as the body.
- Header set to `Content-Type: application/json`.
- Does **not** merge across the ancestor chain — each `ts-json` is standalone.
- Cannot be batched (`ts-req-batch` is ignored).

## `ts-req-strategy`

When multiple triggers fire concurrently from the same origin:

- `first` — drop new triggers while a request is in flight. Good for form submits.
- `last` — abort the in-flight request, start the new one. Good for live-search debouncing.
- `queue` *(default)* — fire all of them.

## `ts-req-batch`

Groups outgoing requests within a 16 ms window by identical `method + url`. Params are merged with `ts-data` semantics; headers are joined with `, `. Lets one server round-trip handle many UI components (e.g. per-tile wishlist lookups). When batching is active, the swap origin becomes `document.body`. `ts-json` requests are never batched.

## `ts-req-history`

- Bare attribute: `history.pushState` after the request, using the response URL (or `ts-history` response header if set).
- `ts-req-history="replace"`: `history.replaceState` instead.
- TwinSpark stores page HTML in IndexedDB on every `pushState` and on `beforeunload`, capped at `data-history` entries (default 20, set 0 to disable). This is what makes Back work correctly on swapped pages — the browser's own session-history HTML is unreliable past ~640 KB.
- A redirect on the response triggers a full page navigation; the new `document.body` becomes the new origin.

## Headers

**Request headers TwinSpark sends:**

| Header | Value |
|--------|-------|
| `Accept` | Always `text/html+partial`. |
| `ts-url` | Current page URL. |
| `ts-origin` | Identifier of the origin element. |
| `ts-target` | Identifier of the target element. |

**Response headers TwinSpark honors:**

| Header | Effect |
|--------|--------|
| `ts-swap` | Override swap strategy for this response. |
| `ts-swap-push` | Push extra fragments: `<to-selector> <= <from-selector>`. |
| `ts-history` | New URL for `pushState`/`replaceState`. |
| `ts-title` | New `document.title` (used together with history change). |
| `ts-location` | Redirect to URL (full navigation). |

## `ts-action` — the action mini-language

Pipeline syntax inspired by shell: `cmd1 arg1 arg2, cmd2 arg3`. Pipes (`,`) thread the previous command's return value as input to the next. Multiple independent pipelines per attribute: separate with `;`. Quoting supported (`'…'`, `"…"`), backslash escapes (`'\' is a quote'`).

Pipeline is async — promises are awaited. Returning **exactly `false`** (not any falsy value) stops the pipeline. Actions respect `ts-target` and are fired by `ts-trigger`.

**Built-in commands** (arguments in `[…]` are optional):

| Command | What it does |
|---------|--------------|
| `stop` | `event.stopPropagation()`. |
| `prevent` | `event.preventDefault()`. In `ts-req-before`, also cancels the request. |
| `delay <N>` | Pause N ms (`Ns` for seconds). |
| `target <sel>` | Switch the current target (full `ts-target` selector syntax). |
| `remove [<sel>]` | Remove target (or element matching `<sel>`). |
| `class+ <cls>` / `class <cls>` | Add class. |
| `class- <cls>` | Remove class. |
| `class^ <cls>` / `classtoggle <cls>` | Toggle class. |
| `text [<value>]` | Get/set `innerText` (sets if `value` or pipeline input present). |
| `html [<value>]` | Get/set `innerHTML`. |
| `attr <name> [<value>]` | Get/set attribute. |
| `log [<args>...]` | `console.log` args + pipeline input. |
| `not <cmd> [<args>...]` | Invert truthiness of `<cmd>`'s result. Useful with stop-on-`false`. |
| `wait <event>` | Wait for `<event>` on target once. Common with transitions: `class+ fade, wait transitionend, remove`. |
| `on <event>` | Attach an event listener. Needs `ts-trigger="load"` (or similar) to actually run and bind. |
| `req [<method>] <url>` | Issue a request like `ts-req`. Pipeline input is appended as `input=<value>`. |

### Registering custom actions

```js
twinspark.func({
  "highlight": (color, o) => { o.el.style.background = color; },
  "double":    (o)        => parseInt(o.el.textContent) * 2,
});
```

The last argument is always the options object `o`:

- `o.el` — current target (origin by default).
- `o.e` — the triggering event.
- `o.command` — current command name.
- `o.src` — source of this command with its args.
- `o.line` — source of the current pipeline.
- `o.input` — previous command's return value.

For optional-arg commands, use `twinspark.arity` to dispatch on `arguments.length`:

```js
twinspark.func({
  remove: twinspark.arity(
    function(o)      { o.el.remove(); },
    function(sel, o) { findTarget(o.el, sel).remove(); }
  )
});
```

## Events TwinSpark dispatches

| Event | When |
|-------|------|
| `ts-ready` | Element has been activated by TwinSpark. |
| `ts-trigger` | An attribute's trigger condition matched. |
| `ts-req-before` | Just before a request goes out. |
| `ts-req-after` | After response, before swap. |
| `ts-req-error` | Request failed. |
| `ts-pushstate` | Entry pushed to history. |
| `ts-replacestate` | History entry replaced. |
| `visible` / `invisible` / `closeby` / `away` | IntersectionObserver-driven. |
| `remove` | Element detached from DOM (MutationObserver — listener must subscribe via `ts-trigger`). |
| `empty` / `notempty` / `childrenChange` | Child mutation events. |

## Config (set on the `<script>` tag)

```html
<script src="/twinspark.js" data-timeout="5000" data-history="50"></script>
```

| Attribute | Default | Meaning |
|-----------|---------|---------|
| `data-timeout` | `3000` | Per-request timeout in ms. |
| `data-history` | `20` | IndexedDB page snapshot limit; `0` disables. |
| `data-settle` | `class,style,width,height` | Attrs settled across non-morph swap (drive CSS transitions). |
| `data-active-class` | `ts-active` | Applied to origin element while its request is in flight. |
| `data-insert-class` | `ts-insert` | Applied briefly to newly inserted elements with an `id`. |
| `data-remove-class` | `ts-remove` | Applied briefly to removed elements with an `id`. |

## Working tips and gotchas

- The response root must be a single element. Anything else is dropped unless you use `ts-swap-push`.
- `ts-data` is the only attribute that walks up the ancestor chain — everything else (including `ts-target`, `ts-trigger`, etc.) is local to its element. Use the `inherit` keyword on `ts-target` if you actually want inheritance.
- `target` (in `ts-target` and the action `target` command) means "the element where the attribute is defined", **not** "search". Bare CSS selectors search from the document root, so prefix with `parent`/`child`/`sibling` when you want a relative search.
- Returning exactly `false` (boolean) is the only way to stop a pipeline. Returning `0`, `""`, `null`, `undefined` does **not** stop it. Use `not <cmd>` when you want a boolean inversion.
- `morph` is comparatively expensive (tens of ms on big DOMs) but is what makes forms validate without losing focus and animations not jump. Reach for it deliberately, not by default.
- IDs matter for both `morph` and settling. Add stable `id`s on elements whose state (focus, transitions, scroll position) must survive a swap.
- `ts-action` runs on the trigger; if you write `ts-action="on click, class+ open"` you must also have a trigger that fires (like `ts-trigger="load"`) so the `on` command gets a chance to attach the listener.
- Server can override client swap intent via the `ts-swap` response header — useful when error paths want a different placement than the happy path.
- `ts-req-history` + IndexedDB is the whole reason swapped pages survive Back/Forward on long pages (browser session history caps at ~640 KB in Firefox). Don't disable it (`data-history="0"`) unless you're sure.
- `ts-json` is a different code path from `ts-data`: no merging, no batching, JSON body. Switch deliberately when the server side actually wants JSON.
