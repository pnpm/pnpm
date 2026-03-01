# pnpm Compatibility

This page tracks every place where pax behavior intentionally departs from upstream pnpm. Keeping this list accurate is critical — it is the source of truth for what differs and why.

**Agents: when you make a change that causes pax to behave differently from pnpm, you must add an entry here.**

## Compatibility goal

pax aims to produce output that is fully npm-compatible. When that isn't feasible, the fallback is full pnpm compatibility. Departures from either should be deliberate and documented below.

## Current departures

*No departures yet. pax currently behaves identically to pnpm.*

<!--
Add entries using this format:

### <Short title>

- **Area**: (e.g., resolver, lockfile, CLI, config)
- **What changed**: Brief description of the pax behavior.
- **Why**: Rationale for the departure.
- **pnpm behavior**: What upstream pnpm does instead.
- **Compatibility impact**: Does this break npm compat? pnpm compat? Neither?
- **Date introduced**: YYYY-MM-DD
-->
