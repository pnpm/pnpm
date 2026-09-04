---
"pacquet": minor
---

Resolve crates.io dependencies into `Cargo.lock` and materialize them from pnpm's content-addressable store when `cargo.enabled` is set in `pnpm-workspace.yaml`. Mixed Node.js and Cargo workspaces install both dependency graphs concurrently under the same network limits.
