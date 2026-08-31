---
"pacquet": patch
---

Load pnpmfile `updateConfig` hooks before packing so hook-provided catalogs resolve in `pnpm pack`, `pnpm publish`, and `pnpm stage publish` [pnpm/pnpm#14377](https://github.com/pnpm/pnpm/issues/14377).
