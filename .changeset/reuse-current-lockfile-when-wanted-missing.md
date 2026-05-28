---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.context": patch
"pnpm": patch
---

Skip dependency re-resolution when `pnpm-lock.yaml` is missing but `node_modules/.pnpm/lock.yaml` exists and still satisfies the manifest. `pnpm install` now reuses the materialized snapshot to regenerate `pnpm-lock.yaml` instead of walking the registry to rebuild it from scratch, turning the cache+node_modules variation into a near-no-op for users who deleted the lockfile but kept the install [#11993](https://github.com/pnpm/pnpm/issues/11993).

`--frozen-lockfile` still refuses to proceed when `pnpm-lock.yaml` is absent — the regenerated lockfile must be committed, so failing loudly is the correct behavior for CI.
