---
"@pnpm/pnpr": minor
"pacquet": minor
---

`pnpm install` now resolves Cargo dependencies through the pnpr server configured in `pnprServer`, as it already did for npm dependencies. pnpr walks the crates.io sparse index and answers with the `Cargo.lock`, so the client no longer fetches one index file per crate in the graph, and every client shares the index files the server has already read. The request carries the workspace's dependency graph alone, without the local paths `cargo metadata` reports. A pnpr server that does not serve Cargo resolution says so during the handshake, and pnpm resolves Cargo dependencies locally as before.
