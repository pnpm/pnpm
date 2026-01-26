---
"@pnpm/package-store": minor
"pnpm": minor
---

Added mark-and-sweep garbage collection for global virtual store.

`pnpm store prune` now removes unused packages from the global virtual store's `links/` directory. The algorithm:

1. Scans all registered projects for symlinks pointing to the store
2. Walks transitive dependencies to mark reachable packages
3. Removes any package directories not marked as reachable

This includes support for workspace monorepos - all `node_modules` directories within a project (including those in workspace packages) are scanned.
