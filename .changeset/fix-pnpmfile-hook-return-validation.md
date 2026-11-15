---
"@pnpm/hooks.pnpmfile": patch
"pnpm": patch
"pacquet": patch
---

`updateConfig` and `afterAllResolved` hooks in `.pnpmfile.cjs` that return `undefined`, `null`, or a non-object value now fail with `ERR_PNPM_CONFIG_IS_UNDEFINED` and `ERR_PNPM_BAD_AFTER_ALL_RESOLVED_RETURN`.
