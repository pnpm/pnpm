---
"@pnpm/deps.graph-builder": patch
"@pnpm/deps.graph-hasher": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.linking.hoist": patch
"@pnpm/store.controller": patch
"pnpm": patch
"pacquet": patch
---

Fixed packages in the global virtual store failing to resolve dependencies that are visible from the project.
