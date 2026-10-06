---
"@pnpm/napi": patch
---

`rebuild()` checks only the lockfile's `patchedDependencies` against the options, as `pnpm rebuild` did in pnpm v11. It no longer fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` on a lockfile that records a `pnpmfileChecksum`, or that was resolved with other settings, because a rebuild resolves nothing.
