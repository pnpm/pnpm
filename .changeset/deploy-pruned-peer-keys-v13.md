---
"pacquet": patch
---

`pnpm deploy --prod` now succeeds with `lockfile.format: v13` when a production package's optional peer is provided only by development dependencies. The excluded provider stays out of the deployed project [pnpm/pnpm#18117](https://github.com/pnpm/pnpm/issues/18117).
