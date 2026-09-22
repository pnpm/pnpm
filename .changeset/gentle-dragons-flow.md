---
"@pnpm/config.reader": patch
"pnpm": patch
"pacquet": patch
---

pnpm now ignores a `"//"` comment key in `overrides`, including the per-project `overrides` in `packageConfigs`. Such a key used to fail with `ERR_PNPM_INVALID_SELECTOR` [#6618](https://github.com/pnpm/pnpm/issues/6618).
