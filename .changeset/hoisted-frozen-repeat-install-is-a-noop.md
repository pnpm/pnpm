---
"pacquet": patch
---

A repeat `pnpm install --frozen-lockfile` with `nodeLinker: hoisted` in a workspace no longer re-links `node_modules` when nothing changed.
