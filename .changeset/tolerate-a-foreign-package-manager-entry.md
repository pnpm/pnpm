---
"pacquet": patch
"pnpm": patch
---

`pnpm install --frozen-lockfile` no longer fails with `ERR_PNPM_FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE` when `pnpm-lock.yaml` records the pinned pnpm version alongside an engine package the running pnpm does not install it from — the block a pnpm older than 11.20.0 writes for a pnpm 12 pin. An entry pinning any other version is still refused, and a writable install still rewrites the block [#14124](https://github.com/pnpm/pnpm/issues/14124).
