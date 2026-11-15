---
"pacquet": patch
---

`updateConfig` hooks in `.pnpmfile.cjs` that return `undefined`, `null`, or a non-object value now fail with `ERR_PNPM_CONFIG_IS_UNDEFINED`. `finders` that return an empty string no longer match packages in `pnpm list --find-by` or `pnpm why --find-by`.
