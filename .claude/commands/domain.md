---
description: Build the DDD/CQRS/ES/Saga backend (evento) for an integrated feature
argument-hint: <feature-or-mockup-to-back>
---

Use the `domain-integrator` agent to implement the write side, read side, and
process side for:

$ARGUMENTS

Use the `evento` crate (aggregators, projections, subscriptions) and wire the
existing Axum handlers to commands and read models. Follow DDD tactical patterns,
CQRS, event sourcing, and sagas/process managers as appropriate.
