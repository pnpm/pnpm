---
"pacquet": patch
---

`pnpm install` now merges Git conflict markers in `pnpm-lock.yaml`. It parses both sides of the conflict and keeps the versions they locked. A conflict in the config dependencies at the top of the lockfile no longer fails the install with `ERR_PNPM_BROKEN_LOCKFILE` [#14880](https://github.com/pnpm/pnpm/issues/14880).
