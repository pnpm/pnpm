---
"@pnpm/installing.deps-installer": patch
---

The `resolutionMode` option for `mutateModules` now defaults to `highest` to match the default in `@pnpm/config`. It previously defaulted to `lowest-direct`.
