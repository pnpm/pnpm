---
"@pnpm/pnpr": patch
---

Team rosters, rule overrides, and upstream cache keys now always include the registry ecosystem (`<ecosystem>/<name>`), so moving a registry into an ecosystem group keeps its stored state and cache. Package tombstones now use the package directory layout, and internal storage and cache directories use consistent dot-prefixed names without version suffixes.
