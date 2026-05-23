---
name: web-accessibility
description: Web accessibility (a11y) reference and review checklist aligned with WCAG 2.2 AA. Use whenever writing, editing, or reviewing HTML, templates (Askama/Jinja/etc.), CSS, or client-side JS that affects user-facing markup. Covers semantic HTML, ARIA usage rules (including "no ARIA is better than wrong ARIA"), keyboard support, focus management, forms and validation, images and media, color and contrast, landmarks and headings, dynamic content / live regions, and common anti-patterns. Designed to be invoked by review/fix agents — every section names what to flag and what the fix looks like.
disable-model-invocation: true
---

# Web Accessibility — Review & Fix Reference

This skill is the source of truth when an agent reviews or fixes accessibility in this repo. Target conformance: **WCAG 2.2 Level AA**. When reviewing, name the WCAG success criterion (e.g. "1.3.1 Info and Relationships") so issues are traceable.

## How to use this skill (for reviewing agents)

1. Walk the page/template top-to-bottom, applying the checklist below.
2. For each issue, report: **location** (file:line), **WCAG SC**, **what's wrong**, **suggested fix** (concrete code, not advice).
3. When fixing, prefer **semantic HTML over ARIA**, and **remove** wrong ARIA rather than layering more on top.
4. Verify with: keyboard-only walkthrough, screen reader spot check (VoiceOver / NVDA / Orca), automated scan (axe / Lighthouse / pa11y).

---

## 1. Semantic HTML first (WCAG 1.3.1, 4.1.2)

The native element nearly always beats a div with ARIA. Flag these:

| Anti-pattern                                  | Fix                                                  |
| --------------------------------------------- | ---------------------------------------------------- |
| `<div onclick="…">` acting as a button        | `<button type="button">` (gets focus, Enter, Space)  |
| `<a href="#" onclick>` for an action          | `<button type="button">` — links navigate, buttons act |
| `<span class="heading-xl">`                   | `<h1>`–`<h6>` with CSS for styling                   |
| `<div role="button" tabindex="0">`            | `<button>`                                           |
| `<table>` for layout                          | CSS grid/flex; reserve `<table>` for tabular data    |
| Nested interactive (`<button>` inside `<a>`)  | Split — interactive elements cannot nest             |

**Rule of thumb:** if a native element exists for what you're building (`button`, `a`, `details/summary`, `dialog`, `input`, `select`, `progress`, `meter`), use it.

## 2. ARIA — the five rules

ARIA is a patch for cases HTML can't express. Apply in this order:

1. **No ARIA is better than wrong ARIA.** Stale or contradictory `aria-*` is worse than none.
2. **Don't change native semantics.** No `<button role="link">`, no `<h2 role="presentation">` (use a `<div>` if you need a non-heading).
3. **All interactive ARIA widgets must be keyboard-operable.** A `role="button"` needs `tabindex="0"` *and* Enter/Space handlers.
4. **Don't hide focusable elements with `aria-hidden="true"` or `role="presentation"`.** Either the element is interactive (and exposed) or it isn't (and not focusable).
5. **All interactive elements must have an accessible name.** Check with the accessible name computation (label, aria-label, aria-labelledby, title, alt — in that priority).

### Frequently misused

- `aria-label` on a `<div>` that isn't a landmark or widget — does nothing for most screen readers. Put the label on the actual interactive/landmark element.
- `aria-hidden="true"` on something that contains a focusable child — creates a keyboard trap into nowhere. Either remove the focusable child from the tab order or drop `aria-hidden`.
- `role="button"` with no `tabindex` — not reachable via keyboard.
- `aria-live="assertive"` everywhere — reserve assertive for true interruptions (errors, alerts). Use `polite` for routine updates.
- `aria-describedby` pointing to an element that doesn't exist — silently broken; verify IDs.

## 3. Keyboard support (WCAG 2.1.1, 2.1.2, 2.4.3, 2.4.7)

Every interaction must work without a mouse. Flag if any of these fail:

- **Tab** reaches every interactive element in a logical order (DOM order ≈ visual order).
- **Shift+Tab** reverses it.
- **Enter** activates links and buttons; **Space** also activates buttons (and checkboxes).
- **Esc** closes modals, menus, popovers.
- **Arrow keys** move within composite widgets (menus, tablists, radio groups, listboxes) — not across them.
- **No keyboard trap** (2.1.2): focus can always leave a region without a mouse. The only allowed "trap" is a modal dialog, and it must be escapable with Esc.

### Focus visibility (WCAG 2.4.7, 2.4.11)

- Never `outline: none` without an equivalent `:focus-visible` style.
- Focus indicator must have ≥ 3:1 contrast against adjacent colors and be at least 2 CSS pixels thick (2.4.11 Focus Appearance, AA in WCAG 2.2).
- Custom focus styles using only color change are insufficient — add a ring or underline.

```css
/* Acceptable baseline */
:focus-visible {
  outline: 2px solid #005fcc;
  outline-offset: 2px;
}
```

### `tabindex` rules

- `tabindex="0"` — make a non-interactive element focusable (only do this if you're also handling keys + role).
- `tabindex="-1"` — programmatically focusable (e.g., to move focus into a dialog), not in tab order.
- `tabindex` ≥ 1 — **never use**. Breaks tab order and is a 2.4.3 violation in practice.

## 4. Focus management (WCAG 2.4.3, 3.2.1, 3.2.2)

Focus must go somewhere predictable after every state change.

| Event                              | Focus should go to…                                  |
| ---------------------------------- | ---------------------------------------------------- |
| Modal/dialog opens                 | First focusable element inside (or the dialog itself with `tabindex="-1"`) |
| Modal closes                       | The element that opened it                           |
| In-page route change (SPA)         | The new `<h1>` or main landmark (programmatic focus, `tabindex="-1"`) |
| Inline content expands             | Stay put (don't yank focus on user-initiated reveal) |
| Server-rendered partial swap (TwinSpark, htmx) | Stay put unless the swap removed the focused element — then focus its container |
| Form validation error              | First invalid field, or a summary above the form     |
| Element being focused is deleted   | Move focus to a stable ancestor before removal       |

Never call `.focus()` on page load without a reason — it surprises users.

## 5. Headings and landmarks (WCAG 1.3.1, 2.4.1, 2.4.6)

### Headings

- Exactly one `<h1>` per page (or per `<main>`/`<article>` in HTML5 outline practice — but one-per-page is the safest bet).
- Don't skip levels (h2 → h4 is a flag).
- Heading text describes the section, not the styling. "Welcome" beats "Big bold text".
- Don't use a heading element for visual emphasis — use CSS.

### Landmarks

Every page needs:

- `<header>` (banner) — site header, one per page at the top level.
- `<nav>` — primary navigation (label additional navs: `<nav aria-label="Breadcrumb">`).
- `<main>` — exactly one per page, wraps the primary content. Skip links target this.
- `<footer>` (contentinfo) — site footer.
- `<aside>` (complementary) — sidebars, related links.
- `<section aria-labelledby="…">` — only when it groups content with an accessible name.

Multiple landmarks of the same type need disambiguating labels via `aria-label` or `aria-labelledby`.

### Skip link (WCAG 2.4.1)

First focusable element on the page:

```html
<a href="#main" class="skip-link">Skip to main content</a>
…
<main id="main" tabindex="-1">…</main>
```

The `.skip-link` should be visually hidden until focused (not `display: none` — that removes it from tab order).

## 6. Images, icons, media (WCAG 1.1.1, 1.2.x)

| Image type                                   | Markup                                                      |
| -------------------------------------------- | ----------------------------------------------------------- |
| Informative (conveys meaning)                | `<img src="…" alt="describe the meaning, not the file">`    |
| Decorative (eyecandy only)                   | `<img src="…" alt="">` — empty alt, never omit the attribute |
| Functional (inside a link/button)            | Alt describes the **action**, not the image. `<a href="/cart"><img src="cart.svg" alt="View cart"></a>` |
| Complex (chart, diagram)                     | Short `alt` + long description (visible text or `aria-describedby`) |
| Text inside an image                         | Avoid; if unavoidable, alt = the exact text                 |
| Inline SVG, decorative                       | `<svg aria-hidden="true" focusable="false">…`               |
| Inline SVG, meaningful                       | `<svg role="img" aria-label="…">` or `<title>` as first child |
| Icon font (e.g. ::before)                    | Wrap visible text or add `aria-label`; icon itself `aria-hidden` |

### Video and audio

- Captions for all prerecorded audio in video (1.2.2).
- Transcript for audio-only (1.2.1).
- Audio description for video where visual content isn't conveyed by the audio (1.2.5).
- Auto-playing audio > 3 s must have a pause/stop/volume control (1.4.2).
- No content flashing > 3 times per second (2.3.1).

## 7. Forms (WCAG 1.3.1, 3.3.1–3.3.4, 4.1.2)

### Labels

Every input needs a programmatic label. Flag in this order:

1. Visible `<label for="id">` paired with `id` — preferred.
2. Wrapping `<label>` — acceptable.
3. `aria-labelledby` pointing to visible text — acceptable.
4. `aria-label` — last resort, only when no visible label exists (e.g., icon-only search).
5. `placeholder` is **not** a label. Flag any input that uses placeholder as its only label.

### Grouping and structure

- Radio buttons and related checkboxes → wrap in `<fieldset>` with `<legend>`.
- Required fields → `required` attribute (and a visible "required" or `*` marker explained in form intro).
- Optional fields explicitly marked when the majority is required, or vice versa — don't make users guess.
- Autocomplete: include `autocomplete="…"` on personal-info fields (1.3.5) — `name`, `email`, `tel`, `street-address`, `cc-number`, etc.

### Errors (3.3.1, 3.3.3)

- Identify errors in text, not by color alone.
- Associate each error with its field via `aria-describedby` pointing to the error message id, and set `aria-invalid="true"`.
- Provide a summary at the top of the form when multiple errors exist; link each entry to the offending field.
- On submit failure, move focus to the summary (or first invalid field).

```html
<label for="email">Email</label>
<input id="email" name="email" type="email"
       autocomplete="email" required
       aria-invalid="true" aria-describedby="email-err">
<p id="email-err" class="err">Enter a valid email like name@example.com.</p>
```

### Submission

- Don't disable the submit button until errors are fixed; let the user submit and learn what's wrong.
- Don't auto-submit on input (`onchange` of a select that navigates is a 3.2.2 violation unless warned).
- Destructive submissions (delete, unsubscribe) need a confirmation step (3.3.4).

## 8. Color and contrast (WCAG 1.4.3, 1.4.11)

Minimum AA ratios:

- **Normal text**: 4.5:1 against its background.
- **Large text** (≥ 24 px regular or ≥ 18.66 px bold): 3:1.
- **UI components & graphical objects**: 3:1 (1.4.11) — button borders, form field borders, focus rings, chart strokes that convey data.

Tools to recommend in the fix: WebAIM contrast checker, browser devtools contrast picker.

**Don't rely on color alone** (1.4.1). Required = red AND `*`. Error = red AND icon AND text. Link = colored AND underlined (or with another visual cue beyond color, like bold + hover-underline, though underline is safest).

## 9. Dynamic content & live regions (WCAG 4.1.3)

When content updates without a page reload, screen readers won't notice unless you tell them.

- `aria-live="polite"` — announces after current speech (toasts, status messages).
- `aria-live="assertive"` — interrupts (errors, urgent alerts). Use sparingly.
- `role="status"` — implicit `aria-live="polite"`. Good for "Saved", "Loaded N items".
- `role="alert"` — implicit `aria-live="assertive"`. Good for validation errors that just appeared.

**Live regions must exist in the DOM before content is injected.** If you render an empty `<div role="status">` and later set its text, screen readers announce. If you create the whole `<div role="status">Saved</div>` at once, many won't announce.

### TwinSpark / htmx / partial swaps

After a swap that meaningfully changes the page state:

- If the swap is an inline update (counter, like state), no announcement needed — visible change is enough for sighted users; for SR users, consider a live region.
- If the swap replaces a region (search results, form panel), include a live-region status inside the swapped content, e.g. `<p role="status" class="visually-hidden">12 results loaded</p>`.
- After delete/destroy actions, move focus to a stable surviving element before the original is removed.

## 10. Visually hidden but accessible

Use this utility class for text that should be available to assistive tech but not visible:

```css
.visually-hidden {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}
```

`display: none` and `visibility: hidden` remove the content from the accessibility tree — those are for hiding from everyone.

## 11. Composite widgets

Use the WAI-ARIA Authoring Practices Guide (APG) pattern verbatim when building these. Common ones:

- **Disclosure (show/hide)** — prefer `<details>/<summary>`. If custom, `aria-expanded` on the trigger.
- **Modal dialog** — `<dialog>` element (with `showModal()`), or `role="dialog" aria-modal="true"` with a focus trap.
- **Tabs** — `role="tablist"` / `role="tab"` (with `aria-selected`, `aria-controls`) / `role="tabpanel"`. Arrow keys switch tabs.
- **Menu (button-triggered)** — `aria-haspopup="menu"`, `aria-expanded`, arrow keys to move, Esc to close.
- **Combobox / autocomplete** — significant complexity; follow APG combobox pattern exactly.
- **Toast / snackbar** — `role="status"` (or `role="alert"` for errors), auto-dismiss must allow user to extend (WCAG 2.2.1).
- **Tooltip** — `aria-describedby`; show on focus *and* hover; dismissible with Esc (1.4.13).

Always link to the APG pattern in the fix recommendation rather than reinventing.

## 12. Touch, target size, motion (WCAG 2.5.5, 2.5.8, 2.3.3)

- **Target size** (2.5.8, AA in WCAG 2.2): interactive targets ≥ 24×24 CSS pixels, unless inline in a sentence or sized by the user agent.
- **Pointer gestures** (2.5.1): any path-based or multi-point gesture must have a single-point alternative.
- **Motion** (2.3.3, AAA but worth applying): respect `prefers-reduced-motion`.

```css
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: 0.01ms !important;
    transition-duration: 0.01ms !important;
  }
}
```

## 13. Language, titles, and document basics (WCAG 3.1.1, 3.1.2, 2.4.2)

- `<html lang="en">` (or appropriate code) — required.
- `<title>` describes the page; unique per page.
- Inline language changes: `<span lang="fr">…</span>`.
- Viewport meta should not block zoom: never `user-scalable=no` or `maximum-scale=1` (1.4.4 violation).
- Page must be usable at 200% zoom without horizontal scroll on standard widths (1.4.10 Reflow).

## 14. Tables (WCAG 1.3.1)

- `<th scope="col">` / `<th scope="row">` for headers.
- `<caption>` for the table's accessible name.
- Don't use tables for layout.
- Complex tables (merged headers, irregular shape) — use `headers="…"` referencing `id` on the th cells, or split into simpler tables.

## 15. Common anti-patterns to flag fast

A non-exhaustive list of things to grep / scan for during review:

- `onclick` on `<div>` or `<span>` → likely needs `<button>`.
- `outline: none` / `outline: 0` without a `:focus-visible` replacement nearby.
- `tabindex="1"` (or any positive value).
- `placeholder=` without an associated `<label>`.
- `<img>` missing `alt=`.
- `<a href="#">` or `<a href="javascript:…">`.
- `aria-hidden="true"` on a focusable element (or one containing focusable children).
- `role="button"` without `tabindex="0"`.
- Empty links and buttons (icon-only with no `aria-label` and no visible text).
- Color words used as the only signifier in copy ("click the red button").
- `<h3>` immediately following `<h1>` with no `<h2>` between.
- Multiple `<h1>` or multiple `<main>` per page.
- `<label>` with no `for` and no wrapped input.
- Auto-playing video with sound and no controls.
- `pointer-events: none` covering interactive content.
- Disabled controls with insufficient contrast (still need 3:1 against background for the boundary to be perceivable; text in disabled controls is exempt under 1.4.3, but make it readable anyway).

## 16. Testing protocol (what a review agent should actually run)

1. **Static scan** — axe-core (via `@axe-core/cli`, Playwright, or Lighthouse). Treat any "serious" or "critical" finding as a must-fix.
2. **Keyboard pass** — Tab from the URL bar through every interactive element. Activate each. Then Esc out of every modal/menu.
3. **Screen reader spot check** — at least one of: VoiceOver (macOS, Cmd+F5), NVDA (Windows, free), Orca (Linux). Navigate by headings (H), by landmarks (D in NVDA, VO+U in VoiceOver), by form fields (F).
4. **Zoom** — 200% browser zoom; check for horizontal scroll, clipped text, overlapping content.
5. **Contrast** — devtools contrast picker for every text/background pair and for UI component borders.
6. **Reduced motion** — toggle OS setting, reload, verify animations are suppressed or instant.

Report findings in the format described at the top: location, SC, what's wrong, concrete fix.

---

## Project-specific notes

- This repo uses **TwinSpark** (`ts-*` attributes) for HTML enhancements. After a `ts-swap` that replaces a region, ensure the swapped content includes a live-region status if the change is meaningful to non-sighted users, and re-establish focus if the previously focused element was removed.
- Templates use **Askama**. When inserting user-controlled data into attributes, rely on Askama's default escaping; for `aria-label` / `alt` / `title` containing user content, ensure the value is escaped (the default `{{ value }}` does this).
- For Rust diagnostics emitted while serving these pages, follow the `tracing-logging` skill — don't `println!` accessibility warnings.
