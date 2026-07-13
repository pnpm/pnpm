---
"@pnpm/store.connection-manager": patch
"@pnpm/installing.commands": patch
"@pnpm/global.commands": patch
"@pnpm/engine.pm.commands": patch
"pnpm": patch
---

`pnpm add -g`, `pnpm update -g`, `pnpm setup`, and the self-updater no longer fail with `ERR_PNPM_MISSING_TIME` when `trustPolicy: no-downgrade` or `resolutionMode: time-based` is set in the global config [#12883](https://github.com/pnpm/pnpm/issues/12883). The decision to fetch full registry metadata now lives in one place, and the `no-downgrade` trust policy always requests full metadata (matching the self-updater), since the trust evidence it checks is missing from abbreviated metadata even on registries that include the `time` field.
