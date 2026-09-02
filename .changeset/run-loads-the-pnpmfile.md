---
"pacquet": patch
---

`pnpm run`, `pnpm exec`, `pnpm rebuild`, and the script shortcuts such as `pnpm test` now load the pnpmfile, so `updateConfig` hook settings such as `extraEnv` and `extraBinPaths` reach the scripts they spawn [#14433](https://github.com/pnpm/pnpm/issues/14433).
