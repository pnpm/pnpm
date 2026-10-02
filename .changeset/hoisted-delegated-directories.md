---
"pnpm": patch
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/store.controller-types": patch
"@pnpm/store.controller": patch
---

`pnpm install` with `nodeLinker: hoisted` now refreshes directories supplied by custom fetchers when reinstalling.
