---
description: Run a PageSpeed / Lighthouse review and apply safe perf fixes
argument-hint: [optional scope — route, template, or asset]
---

Use the `pagespeed-reviewer` agent to audit Performance / Best Practices /
perf-adjacent A11y & SEO against a 100/100/100/100 target on mobile and desktop,
and apply safe in-place fixes. Flag higher-risk changes (bundle splits,
middleware additions, image-pipeline changes) before touching them.

Scope: $ARGUMENTS

If no scope is given, review the layout, critical CSS, asset headers, and the
most recently changed routes.
