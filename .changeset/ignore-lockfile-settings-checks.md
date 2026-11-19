---
"@pnpm/installing.deps-installer": minor
"pnpm": patch
---

Added an `ignoreLockfileSettingsChecks` install option to the programmatic API, ported from pnpm v10. When enabled, the `settings` recorded in `pnpm-lock.yaml` are not compared with the current configuration, so a mismatch no longer fails a `--frozen-lockfile` install with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` and no longer forces a re-resolution in other installs.
