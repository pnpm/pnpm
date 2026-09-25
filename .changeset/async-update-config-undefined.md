---
"@pnpm/hooks.pnpmfile": patch
"pnpm": patch
---

An async `updateConfig` hook that resolves to `undefined` now fails with `ERR_PNPM_CONFIG_IS_UNDEFINED`, as a synchronous hook that returns `undefined` already did.
