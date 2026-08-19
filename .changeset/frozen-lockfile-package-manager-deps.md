---
"@pnpm/installing.env-installer": patch
"pnpm": patch
"pacquet": patch
---

Ensure that running with `--frozen-lockfile` throws an outdated lockfile error when `packageManagerDependencies` is out of sync or not resolved in the lockfile, instead of silently updating the lockfile, resolving the bug described in [pnpm/pnpm#14009](https://github.com/pnpm/pnpm/issues/14009).
