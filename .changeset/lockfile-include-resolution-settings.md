---
"pacquet": minor
---

Added the `lockfile.includeResolutionSettings` setting. When it is `true`, `pnpm-lock.yaml` records `autoDedupe`, `dedupeInjectedDeps`, `dedupePeerDependents` and `linkWorkspacePackages`, and installs treat a lockfile that records other values as outdated. A lockfile that records `autoDedupe` is reused by later installs, so `pnpm run` after `pnpm install --frozen-lockfile` no longer starts another install [#16583](https://github.com/pnpm/pnpm/issues/16583).
