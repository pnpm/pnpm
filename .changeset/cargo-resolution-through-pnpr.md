---
"@pnpm/pnpr": minor
"pacquet": minor
---

`pnpm install` now resolves Cargo dependencies through the server configured in `pnprServer`. The client no longer fetches one sparse-index file per crate in the dependency graph. If the server does not serve Cargo resolution, pnpm resolves Cargo dependencies locally.
