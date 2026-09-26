---
"@pnpm/deps.status": patch
"pnpm": patch
"pacquet": patch
---

A repeat `pnpm install` in a workspace with a custom `modulesDir` now takes the up-to-date fast path. Before, pnpm looked for each workspace project's dependencies in `node_modules` and ran a full install every time.
