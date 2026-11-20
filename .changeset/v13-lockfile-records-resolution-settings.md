---
"pacquet": patch
---

A lockfile in the experimental `v13` format now always records `autoDedupe`, `dedupeInjectedDeps`, `dedupePeerDependents` and `linkWorkspacePackages`, as `lockfile.includeResolutionSettings` does for the `v9` format. After `pnpm install --frozen-lockfile` with `autoDedupe`, `pnpm run` no longer starts another install. Run `pnpm install` once to record the settings in a `v13` lockfile written by an earlier version, since `pnpm install --frozen-lockfile` fails on it [#16583](https://github.com/pnpm/pnpm/issues/16583).
