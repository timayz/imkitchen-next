---
description: Integrate an HTML/Tailwind mockup into the Axum + Askama + TwinSpark app
argument-hint: <path-or-description-of-mockup>
---

Use the `mockup-integrator` agent to integrate the following mockup into the app
(Axum + Askama 0.16 + TailwindCSS + TwinSpark, mobile-first):

$ARGUMENTS

Produce the Askama template under `templates/`, the backing struct model, and the
Axum handler. Use static/fake data for any dynamic values so they can be replaced
by real values later.
