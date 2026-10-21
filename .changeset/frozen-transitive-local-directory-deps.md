---
"@pnpm/lockfile.verification": patch
"pnpm": patch
---

`pnpm install --frozen-lockfile` now fails with `ERR_PNPM_OUTDATED_LOCKFILE` when the dependencies of a transitive injected or local directory dependency no longer match the lockfile, even when that dependency is not a workspace importer. Before, only the immediate directory dependency's children were checked, so a stale transitive snapshot was installed silently [#17128](https://github.com/pnpm/pnpm/issues/17128).
