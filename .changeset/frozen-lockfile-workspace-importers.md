---
"pnpm": patch
"pacquet": patch
---

`pnpm install --frozen-lockfile` now validates every workspace project's `package.json` against the lockfile, not just the root one. A stale workspace manifest (or a project missing from `importers`) fails with `ERR_PNPM_OUTDATED_LOCKFILE` instead of exiting 0 and silently ignoring the drifted dependency; the auto-frozen fast path falls through to a fresh resolve in the same situations.
