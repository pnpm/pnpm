---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"@pnpm/napi": minor
---

Added the `ignoreLockfileSettingsChecks` install option to the programmatic API. When it is enabled, pnpm does not compare the `settings` in `pnpm-lock.yaml` with the current configuration, so a frozen install no longer fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` and other installs no longer re-resolve because of a mismatch.
