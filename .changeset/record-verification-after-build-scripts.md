---
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

`pnpm install` and `pnpm ci` now write the lockfile verification record after the dependency build scripts have run. A failed install no longer leaves a verification record behind.
