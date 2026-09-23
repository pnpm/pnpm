---
"pacquet": patch
---

`pnpm view`, `pnpm config`, `pnpm root`, `pnpm prefix`, `pnpm bin`, `pnpm exec`, `pnpm run`, and the script shortcuts such as `pnpm test` no longer create a temporary file in the project directory, so file watchers such as the Nx daemon no longer see a change. `pnpm exec` and `pnpm run` still create it in projects that declare `configDependencies`, which they install into the store.
