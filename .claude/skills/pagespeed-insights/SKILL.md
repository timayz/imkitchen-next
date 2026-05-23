---
name: pagespeed-insights
description: PageSpeed Insights / Lighthouse review & fix reference for achieving 100/100/100/100 (Performance, Accessibility, Best Practices, SEO) on both mobile and desktop. Use whenever writing, editing, or reviewing HTML, templates (Askama/Jinja/etc.), CSS, JS, HTTP headers, build config, image pipelines, or any user-facing markup that affects Lighthouse scores. Covers Core Web Vitals (LCP, INP, CLS) and lab metrics (FCP, TTFB, TBT, Speed Index); render-blocking resources; critical CSS; image optimization (AVIF/WebP, `srcset`, `sizes`, `width`/`height`, lazy-loading, `fetchpriority`); font loading (`font-display`, preload, subsetting); JS strategy (defer/async, code splitting, tree-shaking, third-party isolation); HTTP caching (`Cache-Control`, `ETag`, immutable assets); compression (Brotli/Gzip); HTTP/2-3 and `Early Hints`; `preconnect`/`dns-prefetch`/`preload`/`prefetch`; CSP and security headers; tap-target sizing; viewport and legible font sizes; and how this skill composes with `web-accessibility` (a11y depth) and `seo` (on-page/indexability depth). Designed for review/fix agents — every section names what to flag and what the fix looks like.
disable-model-invocation: true
---

# PageSpeed Insights — Review & Fix Reference

Target: **100/100/100/100** in PageSpeed Insights (PSI) / Lighthouse on both **mobile** (the primary score) and **desktop**. PSI runs Lighthouse against a real load of the URL on Google's infrastructure, throttled to a 4G/slow CPU profile for mobile, and reports both **field data** (Chrome User Experience Report — real users) and **lab data** (this Lighthouse run).

100 in the lab does not guarantee 100 in the field. Field data is the ranking signal (Core Web Vitals). Optimize for the metric definition, not the score.

## How to use this skill (for reviewing agents)

1. Walk the page/template/headers top-to-bottom, applying the checklist below. Start with **Performance** (the hardest category to keep at 100), then Accessibility, Best Practices, SEO.
2. For each issue, report: **location** (file:line, header, or build step), **category** (Perf / A11y / BP / SEO), **metric affected** (e.g. "LCP", "CLS", "Render-blocking"), **what's wrong**, **suggested fix** (concrete code, not advice).
3. For deep a11y review, defer to the `web-accessibility` skill. For deep on-page/indexability review, defer to the `seo` skill. **This skill focuses on what PSI specifically scores** — they overlap but PSI's checks are a strict subset with thresholds.
4. Verify with: `npx lighthouse <url> --view --preset=desktop` and `npx lighthouse <url> --view` (mobile is default), plus the live PSI run at `https://pagespeed.web.dev/`. Lab runs are flaky on cold CPU/network — run 3× and take the median.
5. Don't game the score with fake CWV reporting or by hiding content from Lighthouse. The categories below describe **fixes**, not workarounds.

---

# PERFORMANCE — 100/100

The Performance score is a weighted mix of lab metrics. Weights (Lighthouse 10+):

| Metric                          | Mobile weight | Target (good) | What it measures                                              |
| ------------------------------- | ------------- | ------------- | ------------------------------------------------------------- |
| **LCP** (Largest Contentful Paint) | 25%        | **≤ 2.5s**    | When the biggest above-the-fold element finishes painting     |
| **TBT** (Total Blocking Time)      | 30%        | **≤ 200ms**   | Sum of main-thread blocks > 50ms between FCP and TTI          |
| **CLS** (Cumulative Layout Shift)  | 25%        | **≤ 0.1**     | Sum of unexpected layout shift scores during page life        |
| **FCP** (First Contentful Paint)   | 10%        | **≤ 1.8s**    | First text/image painted                                      |
| **Speed Index**                    | 10%        | **≤ 3.4s**    | How quickly the visible area is filled                        |

**Field metric** (the one Google uses for ranking, not part of Lighthouse score): **INP** (Interaction to Next Paint, ≤ 200ms). Optimize alongside TBT — the same fixes help both.

To hit 100, every metric needs to land in the "good" band with **margin**, because lab runs are noisy.

## 1. LCP — Largest Contentful Paint

The LCP element is usually the hero image or the largest above-the-fold text block. Find it in the Lighthouse report ("Largest Contentful Paint element") before optimizing.

### Flag these

| Anti-pattern                                                | Fix                                                                                                |
| ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| LCP image is lazy-loaded (`loading="lazy"`)                 | Remove `loading="lazy"` from above-the-fold images. Lazy-loading the LCP element **breaks LCP**.   |
| LCP image has no `fetchpriority="high"`                     | Add `fetchpriority="high"` to the LCP `<img>`. Tells the browser to fetch it before other images.  |
| LCP image fetched via CSS `background-image`                | Use `<img>` instead — the preload scanner sees `<img src>` early; it does not parse CSS background URLs until the stylesheet loads. |
| LCP image discovered late (inside hydrated component)       | Server-render the `<img>` in initial HTML, or add `<link rel="preload" as="image" href="…" fetchpriority="high">` in `<head>`. |
| LCP image served as JPEG/PNG when AVIF/WebP available       | Serve AVIF with WebP fallback. Use `<picture>` with `<source type="image/avif">`.                  |
| Render-blocking stylesheet delays LCP                       | Inline critical CSS in `<head>`, defer the rest. See §4.                                           |
| Web font swaps in late, shifting/blocking LCP text          | `font-display: swap` + preload the font file. See §5.                                              |
| LCP text behind a JS-rendered component                     | Server-render the hero text block. JS-rendered LCP almost always fails the 2.5s budget on mobile. |
| Hero image dimensions much larger than displayed size       | Resize at build time; serve via `srcset` + `sizes` to match viewport.                              |
| TTFB > 600ms                                                | Fix server-side (cache HTML at the edge, optimize DB queries, use HTTP/2-3). LCP can't be < TTFB + render. |

### Preload the LCP image (when discovered late)

```html
<link
  rel="preload"
  as="image"
  href="/hero.avif"
  type="image/avif"
  fetchpriority="high"
  imagesrcset="/hero-480.avif 480w, /hero-960.avif 960w, /hero-1920.avif 1920w"
  imagesizes="100vw">
```

Only preload **one** image (the LCP element). Preloading more competes with the LCP fetch for bandwidth.

### The LCP cookbook

```html
<!-- 1. Above-the-fold hero image -->
<picture>
  <source type="image/avif" srcset="/hero-480.avif 480w, /hero-960.avif 960w, /hero-1920.avif 1920w" sizes="100vw">
  <source type="image/webp" srcset="/hero-480.webp 480w, /hero-960.webp 960w, /hero-1920.webp 1920w" sizes="100vw">
  <img
    src="/hero-960.jpg"
    srcset="/hero-480.jpg 480w, /hero-960.jpg 960w, /hero-1920.jpg 1920w"
    sizes="100vw"
    width="1920"
    height="1080"
    alt="Descriptive alt text"
    fetchpriority="high">
</picture>
```

Notes:
- `width` and `height` are intrinsic dimensions. They reserve aspect-ratio space → prevents CLS. Required even when CSS sets actual size.
- No `loading="lazy"` on the LCP image. Default eager loading is correct here.
- `fetchpriority="high"` is the magic word for LCP.

## 2. CLS — Cumulative Layout Shift

CLS measures layout shifts *not caused by user input*. Score = sum of (impact fraction × distance fraction) per shift. A single shift of 30% of the viewport ≈ 0.3, way above the 0.1 budget.

### Flag these

| Anti-pattern                                                | Fix                                                                                                |
| ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `<img>` without `width` and `height` attributes             | Always set both. Browser computes `aspect-ratio` to reserve space before the image loads.          |
| `<iframe>`, `<video>`, `<embed>` without dimensions         | Same — set `width`/`height` or wrap in a container with `aspect-ratio: 16 / 9`.                    |
| Web font swap shifts surrounding text                       | Match fallback metrics with `size-adjust`, `ascent-override`, `descent-override`. See §5.          |
| Ads/banners/embeds injected without reserved space          | Reserve a fixed-height container (`min-height`). Better: lazy-load below the fold.                 |
| Cookie banner / consent UI pushes content down              | Render as `position: fixed` overlay, not as a block in the document flow.                          |
| Skeleton swapped for content of different height            | Skeleton should match final content height. Use the same `aspect-ratio`.                           |
| Late-loaded CSS changes layout after FCP                    | Inline critical CSS so first render matches final layout.                                          |
| Content inserted above existing content on user-independent event | Insert below, or use `transform` (transforms don't trigger layout shift).                    |
| `position: relative; top: …` animations                     | Animate `transform: translateY(…)` instead. Transforms don't shift layout.                         |

### Aspect-ratio reservation (modern, no `width`/`height` attrs)

```css
.video-embed {
  aspect-ratio: 16 / 9;
  width: 100%;
}
```

```html
<div class="video-embed">
  <iframe src="…" loading="lazy"></iframe>
</div>
```

But for `<img>`, **also** set the attributes — Lighthouse explicitly checks for them.

## 3. TBT / INP — Main thread blocking

TBT is the lab proxy for INP. Both fail when JS hogs the main thread.

### Flag these

| Anti-pattern                                                | Fix                                                                                                |
| ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| Large render-blocking JS in `<head>`                        | Move to `<body>` end, or use `defer` (`<script src="…" defer></script>`).                          |
| `<script>` without `defer` or `async`                       | Add `defer` for scripts that depend on the DOM; `async` for independent third-party tags.          |
| Synchronous third-party scripts (analytics, chat, ads)      | Load async; better: lazy-load on interaction; best: replace with self-hosted lighter alternative.  |
| Long tasks (> 50ms) on the main thread                      | Break into smaller chunks with `scheduler.yield()`, `setTimeout(0)`, or move to a Web Worker.       |
| Hydration of full page on load (SPA frameworks)             | Use island/partial hydration (Astro, Qwik, HTMX/TwinSpark) — ship less JS.                         |
| Megabytes of polyfills shipped to modern browsers           | Use `<script type="module">` / `nomodule` to ship modern JS to modern browsers.                    |
| Unused JS (> 20kb)                                          | Tree-shake; code-split routes; remove dead imports. Check Lighthouse "Reduce unused JavaScript".   |
| Large bundle from a tiny utility (e.g. importing all of `lodash`) | Import named members: `import debounce from 'lodash/debounce'`. Or use modern equivalents.   |
| Heavy `useEffect` / `mounted` work on every page            | Defer to `requestIdleCallback`; gate behind user interaction; cache results.                       |

### `defer` vs `async`

| Attribute | Order preserved? | Executes when?               | Use for                                       |
| --------- | ---------------- | ---------------------------- | --------------------------------------------- |
| (none)    | yes              | parse-blocking, immediately  | Never — always defer or async.                |
| `defer`   | yes              | after HTML parsed, before `DOMContentLoaded` | App JS that touches the DOM.       |
| `async`   | no               | as soon as downloaded        | Independent third-party tags (analytics).     |
| `type="module"` | yes (implicit defer) | after HTML parsed     | Modern ESM app code.                          |

## 4. Render-blocking resources

CSS in `<head>` blocks render until parsed. Big stylesheets = late FCP/LCP.

### Flag these

| Anti-pattern                                                | Fix                                                                                                |
| ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| Single huge `style.css` (200kb+) blocking render            | Inline critical-path CSS in `<head>`; load full stylesheet async (see snippet below).              |
| External font CSS from Google Fonts in `<head>`             | Self-host fonts; or preconnect + preload the woff2 file directly.                                  |
| Multiple `<link rel="stylesheet">` for unrelated routes     | Bundle per-route; only ship the CSS the current page needs.                                        |
| Print stylesheet loaded synchronously                       | `<link rel="stylesheet" href="print.css" media="print">` — non-blocking for screen.                |

### Async-load non-critical CSS

```html
<!-- Critical CSS inline -->
<style>/* above-the-fold rules only — keep under ~14kb */</style>

<!-- Full stylesheet, non-blocking -->
<link rel="preload" as="style" href="/styles.css" onload="this.onload=null;this.rel='stylesheet'">
<noscript><link rel="stylesheet" href="/styles.css"></noscript>
```

Or, with modern syntax:

```html
<link rel="stylesheet" href="/styles.css" media="print" onload="this.media='all'">
```

## 5. Fonts

Web fonts are a top-3 source of LCP delay and CLS.

### Flag these

| Anti-pattern                                                | Fix                                                                                                |
| ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| No `font-display` (default is `block` → invisible text up to 3s) | Set `font-display: swap` in `@font-face`.                                                     |
| Font file not preloaded → discovered after CSS parse        | `<link rel="preload" as="font" type="font/woff2" href="/fonts/x.woff2" crossorigin>`               |
| Loading 6+ font weights/styles                              | Use a variable font (one file, all weights) or load only the weights actually used.                |
| TTF/OTF served when WOFF2 exists                            | WOFF2 only. ~30% smaller than WOFF, ~50% smaller than TTF.                                         |
| No fallback matching → CLS when font swaps                  | Use `size-adjust`, `ascent-override`, `descent-override`, `line-gap-override` on the fallback.     |
| Loading Google Fonts via stylesheet `@import`               | Self-host; or use `<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>` + direct font preload. |
| Unsubsetted font with all glyphs (Latin + Cyrillic + …)     | Subset to the glyphs you use (`unicode-range` in `@font-face`).                                    |

### Reference `@font-face`

```css
@font-face {
  font-family: 'Inter';
  src: url('/fonts/Inter-Variable.woff2') format('woff2-variations');
  font-weight: 100 900;
  font-display: swap;
  font-style: normal;
  unicode-range: U+0000-00FF, U+0131, U+0152-0153, U+02BB-02BC, U+02C6, U+02DA, U+02DC, U+2000-206F, U+2074, U+20AC, U+2122, U+2191, U+2193, U+2212, U+2215, U+FEFF, U+FFFD;
}

/* Match fallback metrics → minimal CLS on swap */
@font-face {
  font-family: 'Inter-fallback';
  src: local('Arial');
  size-adjust: 107%;
  ascent-override: 90%;
  descent-override: 22%;
  line-gap-override: 0%;
}

body {
  font-family: 'Inter', 'Inter-fallback', sans-serif;
}
```

Preload:

```html
<link rel="preload" as="font" type="font/woff2" href="/fonts/Inter-Variable.woff2" crossorigin>
```

`crossorigin` is required for font preload even if same-origin — fonts are always fetched in CORS mode.

## 6. Images

Images dominate page weight. Treat them as the first place to look for performance wins.

### Flag these

| Anti-pattern                                                | Fix                                                                                                |
| ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| JPEG/PNG only, no AVIF/WebP                                 | Generate AVIF (best) and WebP (broad support) at build time; serve via `<picture>`.                |
| Single resolution for all viewports                         | Generate `srcset` (e.g. 480w / 960w / 1440w / 1920w) with `sizes`.                                 |
| No `width`/`height` on `<img>`                              | Always set. Required for CLS prevention and Lighthouse "image elements have explicit width and height". |
| Below-the-fold images load eagerly                          | Add `loading="lazy"` to all below-the-fold images. **Never** on the LCP image.                     |
| Above-the-fold images have `loading="lazy"`                 | Remove `loading="lazy"`. Lazy-loading defers the load past LCP → fails LCP.                        |
| `decoding="sync"` on non-critical images                    | Add `decoding="async"` to decouple decode from main thread.                                        |
| Decorative images use `<img>` with empty alt or no alt      | Use `<img alt="">` (empty string — marks as decorative) or CSS `background-image`.                 |
| SVG icons inlined as huge data URIs in CSS                  | Use SVG sprites or external SVG files; data URIs bloat CSS and can't be cached separately.         |
| Animated GIF                                                | Convert to MP4/WebM with `<video autoplay muted loop playsinline>`. Typically 10× smaller.         |

### Image checklist

```html
<img
  src="/photo-960.jpg"
  srcset="/photo-480.jpg 480w, /photo-960.jpg 960w, /photo-1440.jpg 1440w"
  sizes="(max-width: 600px) 100vw, 50vw"
  width="1440"
  height="900"
  alt="A clear, descriptive alt text"
  loading="lazy"
  decoding="async">
```

For the LCP image, drop `loading="lazy"`, add `fetchpriority="high"`.

## 7. Network — caching, compression, HTTP/2-3

| Anti-pattern                                                | Fix                                                                                                |
| ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| Plain text served uncompressed                              | Enable Brotli (preferred) or Gzip in the server/CDN for `text/*`, `application/javascript`, `application/json`, `image/svg+xml`. |
| Static assets without long `Cache-Control`                  | Fingerprint filename (`/app.a1b2c3.js`) + `Cache-Control: public, max-age=31536000, immutable`.    |
| HTML cached too long                                        | `Cache-Control: no-cache` for HTML (revalidate every load) or short `max-age` with `stale-while-revalidate`. |
| HTTP/1.1 only                                               | Enable HTTP/2 (multiplexing) or HTTP/3 (QUIC). Most CDNs do this by default.                       |
| Multiple subdomains for assets (HTTP/1.1 sharding pattern)  | Consolidate to one origin under HTTP/2. Sharding hurts under H2 (extra connection setup).          |
| No `preconnect` for critical third-party origins            | `<link rel="preconnect" href="https://api.example.com" crossorigin>` for origins you'll fetch from. |
| No `dns-prefetch` fallback for older browsers               | Pair `preconnect` with `dns-prefetch` for older browsers.                                          |

### `preconnect`, `dns-prefetch`, `preload`, `prefetch`

| Hint            | What it does                                       | When                                              |
| --------------- | -------------------------------------------------- | ------------------------------------------------- |
| `preconnect`    | DNS + TCP + TLS handshake for an origin            | Critical third-party origin used soon (fonts, API). Limit to 3-4 — handshakes are expensive. |
| `dns-prefetch`  | DNS lookup only                                    | Backup for `preconnect`; or for less-critical origins. |
| `preload`       | Fetch a specific resource ASAP at high priority    | Late-discovered critical resource (LCP image, hero font). |
| `prefetch`      | Low-priority fetch for the *next* navigation       | User likely to navigate to a known URL next.      |
| `modulepreload` | Preload ESM module + dependencies                  | Critical ESM modules.                             |

Don't overuse. `preload` for everything = `preload` for nothing (browser deprioritizes).

## 8. Third parties

Every third-party tag is a perf risk you don't control.

| Anti-pattern                                                | Fix                                                                                                |
| ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| Google Analytics, Tag Manager, etc. loaded in `<head>` sync | Load `async`. Better: server-side or via a lighter alternative (Plausible, Fathom, self-hosted).   |
| Multiple analytics scripts                                  | One. Pick the one you actually look at.                                                            |
| YouTube `<iframe>` for "click to play" video                | Use the [`lite-youtube-embed`](https://github.com/paulirish/lite-youtube-embed) pattern (poster image + load iframe on click). Saves ~500kb. |
| Chat widget (Intercom, Drift, etc.) loads eagerly           | Lazy-load on user interaction (`click` on a placeholder button).                                   |
| Twitter/Instagram embed pulls in heavy JS                   | Server-fetch the embed HTML; or static image with link.                                            |
| Map embed (Google Maps) loaded on every page                | Use static map image API; load interactive map on click.                                           |

## 9. JS strategy — ship less code

The cheapest JS is the JS you don't ship.

| Anti-pattern                                                | Fix                                                                                                |
| ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| React/Vue/Angular for content sites                         | Static HTML + progressive enhancement (HTMX, TwinSpark, Alpine). 100× less JS.                     |
| Whole-app bundle on landing page                            | Route-based code splitting.                                                                        |
| Polyfills shipped to modern browsers                        | `<script type="module">` for modern, `nomodule` for legacy. Or drop legacy support entirely.       |
| `import * from 'library'` when you use one function         | Named imports + a tree-shaking bundler (esbuild, Rollup, Vite).                                    |
| Source maps shipped to production users                     | Generate but don't reference in production HTML; upload to error monitoring instead.               |
| Moment.js / Lodash in full                                  | `date-fns` / native `Intl`; per-function lodash imports or modern equivalents.                     |

---

# ACCESSIBILITY — 100/100

The Lighthouse Accessibility category runs a subset of [axe-core](https://github.com/dequelabs/axe-core) rules. **It is not a full a11y audit** — 100/100 here does NOT mean WCAG conformant. For full review, use the `web-accessibility` skill.

These are the specific checks PSI Accessibility scores you on:

### Names and labels

| Audit ID                              | What it checks                                                  | Fix                                                                  |
| ------------------------------------- | --------------------------------------------------------------- | -------------------------------------------------------------------- |
| `button-name`                         | Every `<button>` has accessible text                            | Text content, `aria-label`, or `aria-labelledby`.                    |
| `link-name`                           | Every `<a>` has accessible text                                 | Text content; or `aria-label` for icon-only links.                   |
| `image-alt`                           | Every `<img>` has `alt`                                         | Add `alt="…"` (descriptive) or `alt=""` (decorative).                |
| `input-button-name`                   | `<input type="button/submit/reset">` has `value` or `aria-label` | Set `value` (preferred) or `aria-label`.                            |
| `label`                               | Every form `<input>` has a `<label>` or `aria-label`            | `<label for="id">…</label>` + matching `id`; or `aria-labelledby`.   |
| `form-field-multiple-labels`          | An input has more than one label                                | Pick one — multiple `<label for="…">` for the same field.            |
| `frame-title`                         | Every `<iframe>` has `title`                                    | `<iframe src="…" title="Descriptive title">`                         |
| `document-title`                      | Page has a non-empty `<title>`                                  | `<title>Specific page title — Site name</title>`                     |
| `html-has-lang`                       | `<html>` has `lang`                                             | `<html lang="en">` (or correct BCP 47 tag).                          |
| `html-lang-valid`                     | `lang` is a valid BCP 47 tag                                    | Use `en`, `en-US`, `fr`, `de-DE` — not `english`.                    |
| `meta-viewport`                       | Viewport meta tag present and not blocking zoom                 | `<meta name="viewport" content="width=device-width, initial-scale=1">`. Never `user-scalable=no` or `maximum-scale=1`. |
| `object-alt`                          | `<object>` has `alt` or text fallback                           | Add fallback content between `<object>…</object>` tags.              |

### Contrast and visibility

| Audit ID                              | What it checks                                                  | Fix                                                                  |
| ------------------------------------- | --------------------------------------------------------------- | -------------------------------------------------------------------- |
| `color-contrast`                      | Text contrast ≥ 4.5:1 (normal) / 3:1 (large 24px+/18px+ bold)   | Use a contrast checker; adjust the lighter color.                    |
| `target-size` *(WCAG 2.2)*            | Touch targets ≥ 24×24 CSS px (PSI lab) / 44×44 (real mobile)    | Set `min-height`/`min-width`; add padding around small icons.        |

### ARIA

| Audit ID                              | What it checks                                                  | Fix                                                                  |
| ------------------------------------- | --------------------------------------------------------------- | -------------------------------------------------------------------- |
| `aria-allowed-attr`                   | `aria-*` valid for the element's role                           | Remove disallowed attrs; or change the role.                         |
| `aria-required-attr`                  | Required ARIA attrs present for the role                        | E.g. `role="checkbox"` needs `aria-checked`.                         |
| `aria-roles`                          | `role="…"` is a valid ARIA role                                 | Spelling, or remove if no real role applies.                         |
| `aria-valid-attr` / `aria-valid-attr-value` | `aria-*` names and values are valid                       | Fix typos (`aria-labeledby` → `aria-labelledby`).                    |
| `aria-hidden-body`                    | `<body>` does not have `aria-hidden="true"`                     | Remove. Hides everything from AT.                                    |
| `aria-hidden-focus`                   | `aria-hidden` element does not contain focusable descendants    | Move the focusable element out, or remove `aria-hidden`.             |
| `duplicate-id-aria`                   | `id`s used by ARIA refs are unique                              | Make them unique. Loop indexes / route-scoped IDs.                   |

### Structure

| Audit ID                              | What it checks                                                  | Fix                                                                  |
| ------------------------------------- | --------------------------------------------------------------- | -------------------------------------------------------------------- |
| `heading-order`                       | Headings don't skip levels (h1 → h3)                            | Reorder or change tags. h1 → h2 → h3.                                |
| `list`                                | `<ul>`/`<ol>` only contain `<li>`                               | Remove other children, or wrap in `<li>`.                            |
| `listitem`                            | `<li>` is inside `<ul>`/`<ol>`                                  | Wrap in a list, or use a `<div>`.                                    |
| `tabindex`                            | No `tabindex` > 0                                               | Use `0` or `-1` only. Positive `tabindex` breaks tab order.          |
| `bypass`                              | Page provides a way to skip repetitive content                  | "Skip to main content" link as the first focusable element.          |

### Reference

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Page title — Site name</title>
  </head>
  <body>
    <a href="#main" class="sr-only-focusable">Skip to main content</a>
    <header>…</header>
    <main id="main">
      <h1>Page heading</h1>
      …
    </main>
    <footer>…</footer>
  </body>
</html>
```

See the `web-accessibility` skill for deeper WCAG 2.2 AA coverage (keyboard, focus management, live regions, form validation patterns).

---

# BEST PRACTICES — 100/100

PSI's Best Practices category covers security, modern web standards, and avoiding deprecated APIs.

### Security

| Audit ID                              | What it checks                                                  | Fix                                                                  |
| ------------------------------------- | --------------------------------------------------------------- | -------------------------------------------------------------------- |
| `is-on-https`                         | Page served over HTTPS                                          | Enforce HTTPS; 301 redirect HTTP → HTTPS. Use HSTS.                  |
| `csp-xss`                             | Strong Content-Security-Policy header                           | `Content-Security-Policy: …` with `'strict-dynamic'` for scripts. See snippet below. |
| `clickjacking-mitigation`             | `X-Frame-Options` or `frame-ancestors` CSP                      | `Content-Security-Policy: frame-ancestors 'self'` (or `'none'`).     |
| `origin-isolation`                    | `Cross-Origin-Opener-Policy: same-origin`                       | Add the header. Required for `SharedArrayBuffer` and isolates browsing context. |
| `geolocation-on-start` / `notification-on-start` | No permission prompt on page load                    | Trigger from user interaction only.                                  |
| `no-vulnerable-libraries`             | No JS libraries with known CVEs                                 | Update dependencies; check `npm audit` / `snyk`.                     |

### CSP — strict policy

```http
Content-Security-Policy:
  default-src 'self';
  script-src 'self' 'strict-dynamic' 'nonce-{RANDOM}';
  style-src 'self' 'unsafe-inline';
  img-src 'self' data: https:;
  font-src 'self';
  connect-src 'self';
  frame-ancestors 'self';
  base-uri 'self';
  form-action 'self';
  upgrade-insecure-requests;
```

Generate a fresh `nonce` per response. Inline scripts must carry `nonce="{RANDOM}"`.

### Standards / modern web

| Audit ID                              | What it checks                                                  | Fix                                                                  |
| ------------------------------------- | --------------------------------------------------------------- | -------------------------------------------------------------------- |
| `doctype`                             | `<!doctype html>` at the top                                    | First line of every HTML doc.                                        |
| `charset`                             | `<meta charset>` in the first 1024 bytes                        | `<meta charset="utf-8">` right after `<head>`.                       |
| `image-aspect-ratio`                  | Rendered image matches intrinsic aspect ratio                   | Set `width`/`height` to match source; don't stretch.                 |
| `image-size-responsive`               | Images served at appropriate resolution                         | `srcset` + `sizes` so devices get a near-1× DPR image.               |
| `deprecations`                        | No deprecated browser APIs                                      | Read the Lighthouse report; replace deprecated calls.                |
| `errors-in-console`                   | No `console.error` during page load                             | Fix the errors; don't log to `console.error` for non-error info.     |
| `valid-source-maps`                   | If source maps referenced, they're valid                        | Either ship valid maps or remove the `//# sourceMappingURL=` comment. |
| `inspector-issues`                    | No issues from Chrome DevTools issues panel                     | Open DevTools → Issues; fix each (cookies, deprecations, mixed content). |
| `js-libraries`                        | Detects libraries (informational; no points lost)               | —                                                                    |
| `notification-on-start`               | (see Security)                                                  | —                                                                    |
| `paste-preventing-inputs`             | Form inputs don't block paste                                   | Don't preventDefault on `paste` for password / email / OTP inputs.   |
| `third-party-cookies`                 | No third-party cookies                                          | Use first-party cookies; or remove the tracker. Chrome is killing 3p cookies. |

### Other headers worth adding

```http
Strict-Transport-Security: max-age=63072000; includeSubDomains; preload
X-Content-Type-Options: nosniff
Referrer-Policy: strict-origin-when-cross-origin
Permissions-Policy: camera=(), microphone=(), geolocation=(), interest-cohort=()
```

---

# SEO — 100/100

PSI's SEO category is a thin audit — mostly checks that the page is crawlable, has basic meta, and isn't broken. **It is not a full SEO audit.** For deep SEO review (structured data, canonical strategy, internal linking, hreflang, etc.) use the `seo` skill.

These are the audits PSI specifically scores:

### Content / meta

| Audit ID                              | What it checks                                                  | Fix                                                                  |
| ------------------------------------- | --------------------------------------------------------------- | -------------------------------------------------------------------- |
| `document-title`                      | `<title>` exists and non-empty                                  | `<title>Specific page — Site</title>`. 50–60 chars.                  |
| `meta-description`                    | `<meta name="description">` exists and non-empty                | `<meta name="description" content="…">`. 120–160 chars. Unique per page. |
| `http-status-code`                    | Page returned 200                                               | Don't serve real content from 4xx/5xx.                               |
| `link-text`                           | Links have descriptive text (not "click here" / "read more")    | Rewrite anchor text to describe the destination.                     |
| `crawlable-anchors`                   | Links are crawlable (`href`, not JS-only)                       | Every `<a>` must have an `href` to a URL. No `<a onclick>`.          |
| `is-crawlable`                        | Page not blocked by `noindex` or `robots.txt`                   | Remove `<meta name="robots" content="noindex">`; check `robots.txt`. |
| `robots-txt`                          | `robots.txt` is valid (if present)                              | Fix syntax errors; or remove the file.                               |
| `image-alt`                           | (also in A11y)                                                  | —                                                                    |
| `hreflang`                            | `hreflang` values are valid (if present)                        | Use valid ISO 639-1 language + ISO 3166-1 region codes.              |
| `canonical`                           | Canonical URL is valid (if present)                             | Self-referential canonical for standalone pages; valid absolute URL. |
| `structured-data`                     | Manual audit — must be done outside Lighthouse                  | Use Schema.org JSON-LD; validate with [validator.schema.org](https://validator.schema.org). See `seo` skill. |

### Mobile-friendliness

| Audit ID                              | What it checks                                                  | Fix                                                                  |
| ------------------------------------- | --------------------------------------------------------------- | -------------------------------------------------------------------- |
| `viewport`                            | Viewport meta tag present                                       | `<meta name="viewport" content="width=device-width, initial-scale=1">` |
| `font-size`                           | ≥ 60% of text ≥ 12px                                            | Set body font-size to 16px (default) or larger. Avoid <12px anywhere visible. |
| `tap-targets`                         | Touch targets adequately sized + spaced                         | Min 48×48 CSS px for tap targets; 8px gap between adjacent targets.  |

### Reference `<head>`

```html
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Page title — Site name</title>
  <meta name="description" content="A unique 120–160 character description of this page's content.">
  <link rel="canonical" href="https://example.com/this-page">
  <!-- Open Graph + Twitter Card → covered by the `seo` skill -->
</head>
```

---

# Workflow — how to hit 100/100/100/100

1. **Build the page server-rendered** (no JS-only content for indexable/LCP-eligible elements).
2. **Reserve space for everything** — `width`/`height` on images, `aspect-ratio` on embeds. (CLS)
3. **Inline critical CSS, defer the rest.** (FCP, LCP)
4. **Preload the LCP image + hero font.** Set `fetchpriority="high"` on the LCP `<img>`. (LCP)
5. **Lazy-load below-the-fold images** with `loading="lazy" decoding="async"`. (Network)
6. **Serve modern formats** — AVIF/WebP with `srcset`/`sizes`. (Network, LCP)
7. **Defer all scripts.** No sync `<script>` in `<head>`. (TBT)
8. **Brotli + immutable assets + HTTP/2-3.** (Network)
9. **Self-host fonts. `font-display: swap`. Match fallback metrics.** (CLS, LCP)
10. **Headers**: HTTPS + HSTS + CSP + COOP + `X-Content-Type-Options` + `Referrer-Policy`. (Best Practices)
11. **`<head>` essentials**: `lang`, viewport, title, description, canonical, charset. (SEO, A11y)
12. **Alt text, button names, label associations, contrast ≥ 4.5:1.** (A11y)
13. **Test on PSI mobile** (the strictest profile). Median of 3 runs. Then desktop.

---

# Anti-patterns specifically called out

- **Optimizing only desktop**. PSI's headline score is mobile. Mobile passes → desktop passes; the reverse isn't true.
- **Inlining everything to "fix" render-blocking**. Inlined CSS/JS can't be cached. Inline only the **critical path** (above-the-fold styles, maybe 5–14 kb).
- **`loading="lazy"` on the LCP image**. The single most common LCP regression in code review.
- **`preload` for everything**. Browser deprioritizes when overused; you cancel out the benefit.
- **Polyfills via UA sniffing**. Use `<script type="module">` / `nomodule` instead.
- **CWV "fixed" via report manipulation** (not sending real CWV beacons, hiding errors from Lighthouse). Field data comes from real Chrome users — you can't game it long-term.
- **Hitting 100 in the lab, ignoring field**. The CrUX-based field data is what ranks. If field LCP > 2.5s but lab LCP = 1.0s, your real users have a different network/device profile than the lab — investigate.
- **Treating PSI scores as the goal**. The metrics matter. The score is a proxy. A site at 95 with great UX beats a site at 100 that nobody uses.

---

# Companions

- **`web-accessibility`** — full WCAG 2.2 AA review beyond what Lighthouse scores.
- **`seo`** — full on-page/indexability/structured-data review beyond what Lighthouse scores.
- **`twinspark`** — for shipping less JS via HTML-driven enhancement (helps TBT).
- **`askama`** — server-rendered HTML in Rust projects (helps LCP, eliminates hydration TBT).
