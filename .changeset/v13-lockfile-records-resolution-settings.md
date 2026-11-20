---
"pacquet": patch
---

A lockfile in the experimental `v13` format now always records `autoDedupe`, `dedupeInjectedDeps`, `dedupePeerDependents` and `linkWorkspacePackages`, as `lockfile.includeResolutionSettings` does for the `v9` format. After `pnpm install --frozen-lockfile` with `autoDedupe`, `pnpm run` no longer starts another install [#16583](https://github.com/pnpm/pnpm/issues/16583).
