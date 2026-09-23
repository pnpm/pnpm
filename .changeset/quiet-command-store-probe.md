---
"pacquet": patch
---

`pnpm view`, `pnpm config`, `pnpm root`, `pnpm prefix`, `pnpm bin`, `pnpm exec`, `pnpm run`, and the script shortcuts such as `pnpm test` no longer create a temporary file in the project directory when they load their settings. `pnpm exec`, `pnpm run`, and the script shortcuts still create one in projects that declare `configDependencies`.
