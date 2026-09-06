---
"@pnpm/pnpr": minor
"pacquet": minor
---

`pnpm install` now resolves Cargo dependencies through the server configured in `pnprServer`, as it already did for npm dependencies. pnpr walks the crates.io sparse index and returns the `Cargo.lock`. The client no longer fetches one index file per crate in the graph. Every client shares the index files the server has already read. If the server does not serve Cargo resolution, pnpm resolves Cargo dependencies locally.
