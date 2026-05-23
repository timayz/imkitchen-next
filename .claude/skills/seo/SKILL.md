---
name: seo
description: SEO (Search Engine Optimization) reference and review checklist for technical and on-page SEO. Use whenever writing, editing, or reviewing HTML, templates (Askama/Jinja/etc.), routing, sitemaps, robots.txt, or any user-facing markup that affects indexability or ranking. Covers title tags, meta descriptions, canonical URLs, robots directives, Open Graph and Twitter Cards, structured data (JSON-LD / schema.org), heading hierarchy, internal linking, URL design, image SEO, sitemap.xml and robots.txt, hreflang for i18n, Core Web Vitals, mobile-friendliness, and common anti-patterns. Designed to be invoked by review/fix agents — every section names what to flag and what the fix looks like.
disable-model-invocation: true
---

# SEO — Review & Fix Reference

This skill is the source of truth when an agent reviews or fixes SEO in this repo. Target: be **crawlable, indexable, and rankable** by major search engines (Google, Bing, DuckDuckGo) while serving the same content to users and bots (no cloaking). When reviewing, name the category (e.g. "On-page / meta", "Technical / indexability", "Structured data") so issues are traceable.

## How to use this skill (for reviewing agents)

1. Walk the page/template top-to-bottom, applying the checklist below. Start with **indexability** (can the page be crawled and indexed at all?) before optimizing on-page signals.
2. For each issue, report: **location** (file:line), **category**, **what's wrong**, **suggested fix** (concrete code, not advice).
3. Prefer **server-rendered HTML** over JS-injected content for anything that must be indexed. Googlebot renders JS, but with a delay and at lower priority — and other crawlers often don't.
4. Never serve different content to bots vs users (cloaking). It's a manual-action risk and brittle.
5. Verify with: view-source on the rendered page (not the JS-hydrated DOM), Google Search Console URL Inspection, Lighthouse SEO audit, Schema.org validator, robots.txt tester.

---

## 1. Indexability — can the page be seen at all? (must-fix before anything else)

Before touching titles, descriptions, or schema, confirm the page is allowed to be crawled and indexed. The order of precedence search engines apply:

1. **`robots.txt`** — controls *crawling*. A `Disallow` here means the bot won't fetch the page, so any on-page directive (including `noindex`) is invisible. This is a frequent self-own: `Disallow` + `noindex` doesn't deindex — the bot never sees the `noindex`.
2. **HTTP `X-Robots-Tag`** header — same vocabulary as the meta robots tag, applied to any resource (including PDFs, images).
3. **`<meta name="robots">`** — controls *indexing* and link following.
4. **Canonical** — points to the preferred URL; doesn't block indexing, but consolidates ranking signals.

### Flag these

| Anti-pattern                                                | Fix                                                                                                |
| ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `<meta name="robots" content="noindex">` on a page that should rank | Remove. Default behavior is index, follow — no tag needed for that.                                |
| `Disallow: /` in `robots.txt` on a production site          | Restrict to staging only. Production `robots.txt` should `Allow` (or be silent on) public paths.   |
| Blocking JS/CSS in `robots.txt`                             | Allow CSS/JS — Google needs them to render and judge the page.                                     |
| `noindex` AND `Disallow` for the same URL                   | Pick one: `noindex` (let bots fetch + see the directive) OR `Disallow` (block fetching).           |
| Canonical pointing to a different page                      | Canonical to **self** unless the page is a true duplicate/variant of another canonical URL.        |
| `rel="canonical"` to a `noindex` page                       | Broken signal — fix the canonical target or remove `noindex`.                                      |
| Canonical to a non-200 URL (404, redirect)                  | Canonical must be a stable 200.                                                                    |
| Soft 404 (200 status, "not found" body)                     | Return real 404 (or 410 for permanently gone).                                                     |
| Infinite redirect chains, redirects > 2 hops                | Flatten to a single 301.                                                                           |
| Mixing `http://` and `https://` canonicals                  | Canonical to the `https` version; redirect all `http` to `https` (301).                            |
| Trailing-slash inconsistency (`/foo` and `/foo/` both 200)  | Pick one form, 301 the other. Be consistent across the site.                                       |

### Robots meta — full vocabulary

```html
<meta name="robots" content="index, follow">         <!-- default; omit -->
<meta name="robots" content="noindex">                <!-- don't index -->
<meta name="robots" content="nofollow">               <!-- don't follow outgoing links -->
<meta name="robots" content="noindex, nofollow">      <!-- both -->
<meta name="robots" content="noarchive">              <!-- don't show cached copy -->
<meta name="robots" content="nosnippet">              <!-- don't show snippet in SERP -->
<meta name="robots" content="max-snippet:160">        <!-- limit snippet length -->
<meta name="robots" content="max-image-preview:large">
<meta name="robots" content="unavailable_after: 2026-12-31T23:59:59+00:00">
```

Target a specific bot with `<meta name="googlebot" content="…">`; the generic `robots` value applies to all.

## 2. Title tag (highest-leverage on-page signal)

The `<title>` is the single largest on-page ranking signal and the dominant clickable element in the SERP. One per page; unique sitewide.

### Rules

- Length: aim for **50–60 characters** (≈ 580 pixels). Longer gets truncated with "…". Shorter is fine if accurate.
- Front-load the primary keyword/intent: `Primary Keyword — Secondary | Brand`.
- Include the brand at the **end** (separators: `—`, `|`, `·`). For the homepage, brand-first is fine.
- Reflect the page's actual content and search intent — Google rewrites titles that look misleading or stuffed.
- Unique per page. Templated patterns (`<%= product.name %> | Brand`) are fine if `product.name` is itself unique.
- No keyword stuffing (`Buy Shoes, Cheap Shoes, Shoes Online, Shoes Shoes Shoes`).
- No ALL CAPS, no clickbait emojis unless on-brand.

```html
<title>Air Max 90 Running Shoes — Men's | Acme</title>
```

### Flag

- Empty `<title>` or just the site name on every page.
- Duplicate titles across multiple URLs.
- Titles built only from the H1 with no SERP-aware shaping.
- Titles assembled with JS post-render (crawlers may see the pre-render value).

## 3. Meta description

Doesn't directly rank, but drives **click-through rate** from the SERP — which feeds back into ranking via behavioral signals.

- Length: **120–158 characters** (≈ 920 pixels desktop, narrower on mobile). Truncated past that.
- Unique per page. Avoid auto-generated boilerplate.
- Active voice, includes a value proposition + call to action when natural.
- Include the primary keyword once if it fits naturally — Google bolds matching query terms in the snippet.
- If missing, Google generates one from the page. Often fine for long-form content, but for landing/product/category pages, write one.

```html
<meta name="description"
      content="Shop the Air Max 90 in men's sizes 7–14. Free shipping over $75, 60-day returns. Available in 12 colorways.">
```

### Flag

- `<meta name="description" content="">` (empty).
- Same description copied across many pages.
- Truncated at first sentence under 50 characters.
- Description that contradicts the page (bait-and-switch).

## 4. Canonical URLs

Tells search engines the **preferred URL** when the same content is reachable at multiple URLs (tracking params, sort orders, pagination, http/https, www/non-www, trailing slash).

```html
<link rel="canonical" href="https://example.com/products/air-max-90">
```

### Rules

- **Absolute URL**, with scheme and host. Relative canonicals are interpreted but riskier.
- **Self-referential** by default — every indexable page should canonicalize to itself.
- One canonical per page. Multiple `rel="canonical"` tags = search engines ignore all of them.
- Don't canonicalize across languages (use `hreflang` instead).
- Don't canonicalize paginated pages to page 1 — each `?page=N` is its own canonical, or use `rel="next"/"prev"` (deprecated by Google but harmless) plus self-canonicals.
- A canonicalized-away page can still set `noindex`, but prefer one signal: if you want it out of the index, `noindex` is unambiguous.

## 5. Headings — for SEO (overlap with a11y)

Search engines parse heading structure to understand topical hierarchy. Many rules overlap with the `web-accessibility` skill — when both apply, the stricter rule wins.

- One `<h1>` per page, containing the primary topic phrase. Often (but not required to be) similar to the title.
- Use `<h2>`–`<h6>` to outline subtopics in logical order. Don't skip levels.
- Headings should reflect content semantics, not styling. Don't use an `<h2>` because you want bold-large text — use CSS.
- Include relevant terms naturally — don't stuff. "Air Max 90 Sizing Guide" beats "Air Max 90 Sizes Sizing Size Chart".

Crawlers also extract structure from `<section>`, `<article>`, `<nav>`, `<main>` — semantic landmarks help both SEO and accessibility.

## 6. URLs and site structure

URLs are a visible ranking factor (lightly) and a big usability factor.

### Good URLs

- Lowercase, hyphen-separated: `/products/running-shoes/air-max-90`.
- Short and descriptive: `/about` beats `/page?id=42`.
- Hierarchical when there's real hierarchy: `/blog/2026/05/post-slug` only if you genuinely want that structure; flat `/blog/post-slug` is often better.
- Stable. **Don't change URLs** without 301 redirects; you lose accumulated ranking signal otherwise.
- ASCII when possible; otherwise punycode/percent-encoded properly.

### Flag

- Query strings doing the work of a path (`/page?id=42&section=foo` where a path would work).
- Session IDs or tracking params in canonical URLs.
- Underscores instead of hyphens (`/air_max_90`) — hyphens are the word separator search engines understand.
- Mixed case (`/Products/AirMax90`).
- Stop-word noise (`/the-best-of-the-running-shoes`).
- Deep nesting > 4 levels without reason.
- `index.html` or `.php` extensions exposed in user-facing URLs.

## 7. Internal linking

The most undervalued on-page signal. Crawlers discover pages via links, and link anchor text + position teaches them what each page is about.

### Rules

- Every important page should be reachable from the homepage within **3 clicks** (rule of thumb).
- Use **descriptive anchor text** that includes the target page's topic — never `click here`, `read more`, `here`.
  - Bad: `Read more <a href="/guide">here</a>.`
  - Good: `Read our <a href="/guide">sizing guide for running shoes</a>.`
- Link to canonical URLs (not to redirect chains).
- Orphan pages (no internal links pointing to them) won't rank well; flag them.
- Use `rel="nofollow"` for untrusted user-generated content (comments, forums). Use `rel="ugc"` for user-generated content specifically. Use `rel="sponsored"` for paid links. Combine as needed: `rel="nofollow ugc"`.
- Open external links in the same tab unless there's a UX reason not to. If you use `target="_blank"`, always add `rel="noopener"` (security) — `noreferrer` if you don't want to leak the referer (an SEO consideration for the target site).

### Flag

- "click here", "read more", "learn more" as the only anchor text.
- Anchors with images and empty alt → no anchor text for crawlers.
- Massive footer link blocks (200+ links) — dilutes link equity and looks spammy.
- Pages with zero internal inbound links.
- Internal links to redirected URLs (link to the final destination directly).

## 8. Open Graph and Twitter Cards (social preview, indirect SEO)

These don't affect ranking directly but drive shares and clicks, which feed back into authority signals.

### Open Graph (Facebook, LinkedIn, Slack, Discord, iMessage)

```html
<meta property="og:type" content="article">
<meta property="og:title" content="Air Max 90 Running Shoes — Men's">
<meta property="og:description" content="Shop the Air Max 90 in men's sizes 7–14. Free shipping over $75.">
<meta property="og:url" content="https://example.com/products/air-max-90">
<meta property="og:image" content="https://example.com/img/air-max-90-og.jpg">
<meta property="og:image:width" content="1200">
<meta property="og:image:height" content="630">
<meta property="og:image:alt" content="Air Max 90 in red colorway on white background">
<meta property="og:site_name" content="Acme">
<meta property="og:locale" content="en_US">
```

### Twitter (X) Cards

```html
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:site" content="@acme">
<meta name="twitter:creator" content="@author_handle">
<!-- title, description, image inherit from og:* if not set -->
```

### Rules

- Image: **1200 × 630 px**, < 5 MB, JPEG or PNG. Absolute HTTPS URL.
- Title and description can differ from `<title>` and `<meta name="description">` — tune for share context vs. SERP.
- Use `property="…"` for `og:` (it's RDFa); use `name="…"` for `twitter:`. Mixing them is a frequent bug.
- Test with [opengraph.xyz](https://www.opengraph.xyz), Facebook Sharing Debugger, LinkedIn Post Inspector, Twitter Card Validator.

## 9. Structured data (JSON-LD / schema.org)

Structured data unlocks **rich results** (stars, FAQ, breadcrumbs, recipes, products with price) and helps search engines understand entities.

**Format:** prefer **JSON-LD** in a `<script type="application/ld+json">` block. It's what Google recommends and it doesn't pollute the visible HTML.

### Common types to consider

| Page type             | Schema                                                                                |
| --------------------- | ------------------------------------------------------------------------------------- |
| Homepage / brand      | `Organization` (or `LocalBusiness`), `WebSite` with `SearchAction`                    |
| Article / blog post   | `Article` / `NewsArticle` / `BlogPosting`                                             |
| Product detail        | `Product` with nested `Offer` and `AggregateRating`                                   |
| FAQ                   | `FAQPage` with `Question` / `Answer`                                                  |
| How-to                | `HowTo` with `step` array                                                             |
| Recipe                | `Recipe` (very rich result-friendly)                                                  |
| Event                 | `Event` (date, location, offers)                                                      |
| Breadcrumb trail      | `BreadcrumbList`                                                                      |
| Person / author       | `Person`                                                                              |
| Video                 | `VideoObject` (with `thumbnailUrl`, `uploadDate`, `duration`)                         |
| Local business        | `LocalBusiness` (with address, phone, geo, openingHoursSpecification)                 |

### Example — Article

```html
<script type="application/ld+json">
{
  "@context": "https://schema.org",
  "@type": "Article",
  "headline": "How to Pick the Right Running Shoe",
  "image": ["https://example.com/img/hero.jpg"],
  "datePublished": "2026-05-12T08:00:00+00:00",
  "dateModified": "2026-05-20T14:30:00+00:00",
  "author": {
    "@type": "Person",
    "name": "Jane Doe",
    "url": "https://example.com/authors/jane-doe"
  },
  "publisher": {
    "@type": "Organization",
    "name": "Acme",
    "logo": {
      "@type": "ImageObject",
      "url": "https://example.com/logo.png"
    }
  },
  "mainEntityOfPage": "https://example.com/blog/pick-running-shoes"
}
</script>
```

### Rules

- **Match the visible page.** Don't put a 5-star aggregateRating in JSON-LD if the page doesn't show one. Google penalizes structured-data spam.
- Use absolute URLs for `image`, `@id`, `url`.
- Required vs recommended fields: check the [Google Search Central docs](https://developers.google.com/search/docs/appearance/structured-data) for each type; required fields must be present or the rich result won't render.
- One `@type` per `<script>` block — or combine via `@graph`:

  ```json
  { "@context": "https://schema.org", "@graph": [ {...}, {...} ] }
  ```
- Don't mix Microdata + JSON-LD for the same entity; pick one.
- Validate with [Schema Markup Validator](https://validator.schema.org) and Google's [Rich Results Test](https://search.google.com/test/rich-results).

### Flag

- JSON-LD with syntax errors (trailing commas, unescaped quotes) — silently invalid.
- Fake or inflated `aggregateRating` / `reviewCount`.
- `Product` schema missing `offers.price` and `offers.priceCurrency`.
- `Article` missing `datePublished`, `author`, or `headline`.
- `BreadcrumbList` missing `position` integers or with broken URLs.
- Mismatched `@type` (e.g., `Product` schema on a blog post).

## 10. Images — SEO essentials

Images can rank in Google Images and contribute to relevance and Core Web Vitals.

### Rules

- **Descriptive filenames**: `air-max-90-red-mens.jpg` beats `IMG_8472.jpg`.
- **`alt` attribute**: describes the image (same rules as accessibility — see `web-accessibility` skill). Crawlers read alt as the image's accessible name and a relevance signal.
- **Modern formats**: WebP or AVIF for raster; SVG for vector. Fall back via `<picture>`:

  ```html
  <picture>
    <source srcset="hero.avif" type="image/avif">
    <source srcset="hero.webp" type="image/webp">
    <img src="hero.jpg" alt="Air Max 90 in red on white background" width="1200" height="630">
  </picture>
  ```
- Always set **`width` and `height` attributes** (or aspect-ratio CSS) — prevents Cumulative Layout Shift (CLS), a Core Web Vital.
- **Lazy-load below-the-fold** images: `loading="lazy"`. Never lazy-load the LCP (Largest Contentful Paint) image — that hurts performance.
- **Responsive images** with `srcset` and `sizes` to serve the right resolution per viewport.
- Hosted on the same domain (or a CDN under your control) for image-search attribution.
- Include images in a dedicated image sitemap or extend the main sitemap with `<image:image>` entries for image-heavy sites.

### Flag

- `<img>` missing `alt`.
- Hero/LCP image with `loading="lazy"`.
- Images served at 4× their displayed size.
- PNG used where JPEG/WebP would be 10× smaller (photographs).
- No `width`/`height` → layout shift.

## 11. Performance — Core Web Vitals

Google uses Core Web Vitals as a ranking signal. They're measured from real user data (CrUX) and on Lighthouse synthetic runs.

| Metric                              | Target (good) | What it measures                                          |
| ----------------------------------- | ------------- | --------------------------------------------------------- |
| **LCP** Largest Contentful Paint    | ≤ 2.5 s       | Time until the largest visible element renders            |
| **INP** Interaction to Next Paint   | ≤ 200 ms      | Responsiveness to user interactions (replaced FID in 2024)|
| **CLS** Cumulative Layout Shift     | ≤ 0.1         | Visual stability — unexpected layout jumps                |

### Quick wins to recommend

- Server-render the LCP element (hero image, headline) — don't depend on JS.
- Preload critical assets: `<link rel="preload" as="image" href="/hero.avif" fetchpriority="high">`.
- Defer non-critical JS: `<script defer src="…">` or `type="module"`.
- Inline critical CSS for above-the-fold content; load the rest asynchronously.
- Self-host fonts or use `<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>`. Set `font-display: swap` on `@font-face`.
- Set `width`/`height` on images and embeds to reserve space (prevents CLS).
- Avoid layout-shifting ads, late-loading banners, and dynamically injected DOM above existing content.
- Use HTTP/2 or HTTP/3; enable compression (Brotli > gzip); set sensible `Cache-Control`.

### Flag

- Render-blocking `<script>` in `<head>` without `defer` or `async`.
- Webfonts loaded without `font-display: swap` (causes invisible text or FOIT).
- Hero image not preloaded.
- Cookie/consent banners injected without reserved space.

## 12. Mobile-first indexing

Google indexes the **mobile** version of your site by default. The mobile and desktop versions must serve **equivalent content**.

### Rules

- **Responsive design** is preferred. If you ship separate mobile (`m.example.com`) and desktop sites, cross-link with `rel="alternate"` + `rel="canonical"` correctly.
- Same `<title>`, meta description, structured data, and primary content on mobile as on desktop. Don't hide critical content behind "Show more" on mobile only.
- Viewport meta tag is mandatory:

  ```html
  <meta name="viewport" content="width=device-width, initial-scale=1">
  ```
- Don't block CSS/JS that the mobile version needs (overlaps with §1).
- Tap targets ≥ 24×24 CSS px and spaced apart enough to not misfire (overlaps with WCAG 2.5.8).
- Text legible without zoom: ≥ 16 px base font size.

### Flag

- `user-scalable=no` or `maximum-scale=1` (accessibility AND SEO violation — Google flags it as "viewport not configured for mobile").
- Mobile version with fewer headings or stripped content vs desktop.
- Pop-ups/interstitials that obscure mobile content (intrusive interstitial penalty).

## 13. Sitemaps and robots.txt

### `sitemap.xml`

A list of canonical URLs you want indexed. Helps crawlers find pages, especially on large or poorly-linked sites.

```xml
<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url>
    <loc>https://example.com/</loc>
    <lastmod>2026-05-20</lastmod>
    <changefreq>daily</changefreq>
    <priority>1.0</priority>
  </url>
  <url>
    <loc>https://example.com/products/air-max-90</loc>
    <lastmod>2026-05-15</lastmod>
  </url>
</urlset>
```

- Only include **indexable, canonical, 200-status** URLs. No `noindex`, no redirects, no 404s, no non-canonical variants.
- `lastmod` should reflect actual content changes (lying about it loses crawler trust).
- `changefreq` and `priority` are hints; modern Google largely ignores them. `lastmod` is the one to get right.
- Split into multiple sitemaps if > 50,000 URLs or > 50 MB uncompressed; reference them in a `sitemapindex`.
- Reference the sitemap in `robots.txt`: `Sitemap: https://example.com/sitemap.xml`.
- Submit to Google Search Console and Bing Webmaster Tools.

### `robots.txt`

```
User-agent: *
Allow: /
Disallow: /admin/
Disallow: /search?
Disallow: /*?session=

Sitemap: https://example.com/sitemap.xml
```

- Place at the **domain root**: `https://example.com/robots.txt`. Not in a subdirectory.
- Crawl-Delay is honored by Bing, ignored by Google.
- Don't use `robots.txt` to hide sensitive data — it's publicly readable.
- Use it to keep crawlers out of internal search results, filtered URLs, faceted navigation infinite spaces, and bot-trap pagination.

### Flag

- `sitemap.xml` listing redirected or `noindex` URLs.
- `robots.txt` blocking CSS, JS, or images.
- `robots.txt` with `Disallow: /` on a production domain.
- Sitemap submitted from a different domain than its URLs (Google ignores it).
- Missing `<lastmod>` on a sitemap that's expected to drive recrawls.

## 14. Internationalization — `hreflang`

When the same content exists in multiple languages or regional variants, `hreflang` tells search engines which version to show which audience.

```html
<link rel="alternate" hreflang="en"    href="https://example.com/page">
<link rel="alternate" hreflang="en-gb" href="https://example.com/en-gb/page">
<link rel="alternate" hreflang="fr"    href="https://example.com/fr/page">
<link rel="alternate" hreflang="x-default" href="https://example.com/page">
```

### Rules

- Use **ISO 639-1** language codes (`en`, `fr`, `de`), optionally with **ISO 3166-1 alpha-2** region (`en-gb`, `pt-br`). Not country alone.
- **Bidirectional**: each variant must link to all the others, including itself.
- Include `x-default` for the fallback (the version shown when no language matches).
- Canonical and `hreflang` are independent — each variant's canonical is **itself**, not the default-language version.
- Don't use `hreflang` for plain duplicates within the same language.
- Alternative places to declare: HTTP `Link` header, or in `sitemap.xml` via `<xhtml:link rel="alternate">`.

### Flag

- Missing return links (page A → B but B doesn't → A).
- Invalid codes (`hreflang="uk"` for United Kingdom — should be `en-gb`; `uk` is Ukrainian).
- `hreflang` to a canonical that points elsewhere.
- Same `hreflang` value declared for multiple URLs.

## 15. Pagination, faceted navigation, duplicate content

### Pagination

- Each page is its own canonical (`/blog?page=2` → canonical to itself, not to page 1).
- Each page has unique titles where feasible (`Blog — Page 2 | Brand`).
- `rel="next"` and `rel="prev"` are deprecated by Google but still respected by some crawlers — harmless to include.

### Faceted nav / filters

- Filter combinations explode into infinite URLs. Decide which combinations should be indexable.
- For non-indexable filtered URLs: `noindex, follow` (let crawlers walk the links but skip indexing) OR canonical to the unfiltered page OR block via `robots.txt` if there are crawl-budget issues.
- Don't simply 301 filter URLs to the base — users may share them.

### Duplicate content (same-domain)

- `?utm_*` and other tracking params: canonical to the param-free URL.
- HTTP vs HTTPS, www vs non-www, trailing slash: pick one, 301 the others.
- Print-friendly pages: `noindex` or canonical to the main version.
- Staging/dev domains: `noindex` site-wide or HTTP-auth them.

### Duplicate content (cross-domain / syndication)

- Self-canonical with a `link rel="canonical"` pointing back to the original on the source site, OR use the syndicator's tooling.
- Republish with at minimum a visible "Originally published on [link]" credit.

## 16. JS-rendered pages and SPAs

Search engines (Google) render JavaScript, but:

- Rendering is a second pass with a delay (often hours/days for non-priority pages).
- Other crawlers (Bing improving, social previewers, smaller engines) often **don't** execute JS at all.
- If content critical to ranking only appears after `fetch()` + DOM mutation, you're betting your SEO on Googlebot's renderer.

### Recommendations

- **SSR or pre-render** the indexable content. Hydrate on the client.
- For SPAs, use a meta-framework with SSR/SSG (Next.js, Nuxt, SvelteKit, Astro, Remix) or a pre-render service.
- Update `<title>`, meta description, canonical, and structured data **per route** — not just on initial load.
- Don't rely on `#hash` URLs for distinct pages — use real paths with the History API.
- Test by viewing the **raw HTML response** (`curl https://example.com/page | grep -i '<title>'`), not the JS-hydrated DOM.

### Flag

- View-source HTML missing the `<title>`, `<meta>` tags, primary content.
- Pre-render service returning different content from the live site.
- SPA route changes that don't update `document.title` or canonical.

## 17. HTTPS, security, and trust signals

- **HTTPS** is a baseline ranking signal — non-HTTPS pages are deprioritized. No exceptions.
- Valid TLS cert, no mixed content (HTTPS page loading HTTP assets).
- `HSTS` header on HTTPS to enforce the upgrade.
- No expired certificates — browsers show big warnings, crawlers note them.
- Implement security headers (`Content-Security-Policy`, `X-Content-Type-Options: nosniff`, `Referrer-Policy: strict-origin-when-cross-origin`) — not direct ranking factors but signal a well-maintained site and prevent issues that *are* ranking factors (hacked content, malware).

## 18. Crawl budget and crawler hygiene

Mostly relevant for sites with millions of URLs, but worth knowing:

- Eliminate crawler traps: faceted nav generating infinite URLs, calendar widgets with `?date=…` ad infinitum, session-ID URLs.
- Return correct status codes: 200 for OK, 301 for permanent moves, 302 for temporary, 404 for missing, 410 for permanently gone, 503 for planned downtime (with `Retry-After`).
- Use `If-Modified-Since` / `ETag` so crawlers can save bandwidth on unchanged pages.
- Watch Google Search Console **Crawl Stats** and **Coverage** reports for "Discovered — not indexed" and "Crawled — currently not indexed" patterns.

### Flag

- 200 status on pages that are functionally 404 (soft 404).
- 302 used for permanent redirects (use 301).
- Long redirect chains (> 2 hops).
- 500-level errors served for missing pages.

## 19. Content quality — quick heuristics for review

SEO is downstream of content quality. Flag obvious issues:

- **Thin content**: pages with < 100 words of unique copy, especially product/category pages with no description.
- **Duplicate content**: same body text on multiple URLs.
- **Auto-generated / scraped content** that adds no value (Google's helpful-content system targets this).
- **Keyword stuffing**: same term repeated unnaturally.
- **Hidden text** (`color: white on white`, `display: none` containing keyword blocks) — manual-action risk.
- **Missing EEAT signals** on YMYL (Your-Money-or-Your-Life) topics: author bio, citations, last-updated date, qualifications.
- **No content-update mechanism**: stale dates on a site claiming to be current.

## 20. Common anti-patterns to flag fast

Grep / scan for these during review:

- `<title></title>` or missing `<title>`.
- Two `<title>` tags (some templates inadvertently render both a default and an override).
- `<meta name="description" content="">` empty or missing.
- `<link rel="canonical">` missing on indexable pages.
- `<link rel="canonical" href="…">` with a relative URL.
- `<meta name="robots" content="noindex">` on a page that's also in `sitemap.xml`.
- `Disallow: /` in production `robots.txt`.
- `<a href="#">` and `<a href="javascript:…">` as primary navigation links.
- `click here`, `read more`, `learn more` as the only anchor text.
- Inline JS-only navigation (`<div onclick="location='…'">`).
- `<img>` without `alt` or with `alt="image"` / `alt="picture"`.
- `<img>` without `width` / `height`.
- Hero image with `loading="lazy"`.
- Multiple `<h1>` per page; skipped heading levels.
- 200 OK on a "not found" page (soft 404).
- HTTP page (non-HTTPS) linked from HTTPS pages.
- Mixed content (HTTPS page loading HTTP `<img>`, `<script>`, `<iframe>`).
- JSON-LD with trailing commas / single quotes / unescaped HTML.
- JSON-LD `aggregateRating` without matching on-page reviews.
- OG image not exactly 1200×630 or hosted on HTTP.
- `og:` tags using `name="og:title"` instead of `property="og:title"`.
- `hreflang` codes that aren't ISO (e.g., `uk` for UK, `cz` for Czech — should be `en-gb`, `cs`).
- `hreflang` without `x-default` and without return links.
- `viewport` meta with `user-scalable=no`.
- Pagination canonicalized to page 1.
- Tracking-param URLs canonicalized to themselves instead of the clean URL.
- Sitemap listing URLs that 404, 301, or `noindex`.

## 21. Testing protocol (what a review agent should actually run)

1. **View raw HTML** — `curl -sL https://example.com/page` (or open view-source) and verify `<title>`, `<meta>`, canonical, structured data, primary content are all present **before** JS runs.
2. **Lighthouse SEO audit** — Chrome DevTools or `npx lighthouse <url> --only-categories=seo`. Aim for ≥ 95 score.
3. **Rich Results Test** — [search.google.com/test/rich-results](https://search.google.com/test/rich-results) — validates JSON-LD + shows what rich result (if any) the page is eligible for.
4. **Schema validator** — [validator.schema.org](https://validator.schema.org) — catches structural errors Google's tool may miss.
5. **Mobile-Friendly Test** — Google's tool or Lighthouse mobile category.
6. **PageSpeed Insights** — [pagespeed.web.dev](https://pagespeed.web.dev) — CrUX field data + Lighthouse lab data for Core Web Vitals.
7. **`robots.txt` Tester** — Google Search Console.
8. **Sitemap validator** — XML well-formed, all URLs return 200, listed URLs are canonical.
9. **Crawl simulation** — Screaming Frog SEO Spider (free up to 500 URLs), Sitebulb, or a custom crawl — finds broken links, redirect chains, duplicate titles, missing meta.
10. **Search Console** — URL Inspection tool for any specific page to see how Google sees it (rendered HTML, indexing status, mobile usability).
11. **Manual SERP check** — `site:example.com keyword` to see what Google has indexed and how it's snippeting.

Report findings in the format described at the top: location, category, what's wrong, concrete fix.

---

## Project-specific notes

- This repo uses **Askama** templates. SEO tags should live in a base layout block (`{% block head %}`) and be overridable per page — never hard-code titles or canonicals in the layout. Use the default `{{ value }}` escaping for any user-controlled string that ends up in `content="…"` of a meta tag.
- For **TwinSpark** (`ts-*`) partial swaps: a `ts-swap` updates a fragment, not the document. If the swap changes the logical page (e.g., infinite scroll on a feed), update the canonical and title via a full navigation or `pushState`-style mechanism — SEO crawlers won't see the swapped state. For purely interactive components (filters, accordions), partial swaps are fine.
- **i18n**: when adding `hreflang`, ensure return links are present on every translated variant — Askama makes it easy to generate the full set in the base layout from a list of locales.
- **Sitemap generation**: should be a build-time or scheduled job. Include only URLs whose route handlers return 200 and don't set `noindex`. If a route is generated from a database, the sitemap query should filter on the same publish/visibility flags.
- For Rust diagnostics during SEO-related rendering or sitemap generation, follow the `tracing-logging` skill — don't `println!`.
- For commit messages on SEO fixes, follow the `conventional-commits` skill. Most SEO fixes are `fix:` (broken canonical, missing alt) or `feat:` (new schema, new sitemap); content-shaping changes (rewriting a title) may be `chore:` if they don't change behavior, or `feat:` if they materially change the page's targeting.
