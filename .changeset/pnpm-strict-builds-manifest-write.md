---
"pacquet": patch
---

`pnpm add`, `pnpm update`, and `pnpm remove` now save `package.json` before failing with `ERR_PNPM_IGNORED_BUILDS`. The dependency they were asked to change is already materialized by that point, so the manifest has to record it — otherwise the next install removes the packages again.
