---
"pacquet": minor
---

`lockfile.includeResolutionSettings: true` makes `pnpm-lock.yaml` record `autoDedupe`, `dedupeInjectedDeps`, `dedupePeerDependents` and `linkWorkspacePackages`. Installs then treat a lockfile that records other values as outdated. A lockfile that records `autoDedupe` is reused by later installs on any machine, so `pnpm run` after `pnpm install --frozen-lockfile` no longer starts another install [#16583](https://github.com/pnpm/pnpm/issues/16583).
