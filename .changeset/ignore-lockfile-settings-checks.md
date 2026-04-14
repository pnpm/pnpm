---
"@pnpm/core": minor
---

Added a new `ignoreLockfileSettingsChecks` option. When enabled, pnpm skips the validation that compares the `settings` section of `pnpm-lock.yaml` with the current configuration during `--frozen-lockfile` and `--prefer-frozen-lockfile` installs, proceeding as if the settings are up to date.
