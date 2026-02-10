---
"@pnpm/cli-utils": patch
"@pnpm/config": patch
"pnpm": patch
---

Fix `shamefullyHoist` set via `updateConfig` in `.pnpmfile.cjs` not being converted to `publicHoistPattern` [#10271](https://github.com/pnpm/pnpm/issues/10271).
