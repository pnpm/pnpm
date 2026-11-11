---
"@pnpm/installing.commands": patch
"pnpm": patch
---

A repeat `pnpm install` with `nodeLinker: pnp` now applies dependency and configuration changes. Before, it printed "Already up to date" and left the lockfile unchanged.
