---
"pacquet": patch
"@pnpm/napi": patch
"@pnpm/deps.status": patch
"pnpm": patch
---

`pnpm install` now returns "Already up to date" in a workspace where `dedupeDirectDeps` left a project nothing to link into its own `node_modules`. Such a project has all its direct dependencies declared by the root with the same specifiers and never gets a `node_modules` directory; the check treated that as a missing install and ran the full install every time.
