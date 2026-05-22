---
"@pnpm/installing.deps-resolver": patch
pnpm: patch
---

Fixed non-determinism in `pnpm dedupe` and `pnpm install` when a dependency graph contains packages with transitive peer dependencies on each other (e.g. `@aws-sdk/client-sts` and `@aws-sdk/client-sso-oidc`) and `auto-install-peers` is enabled. The lockfile no longer flips between two equally-valid forms across consecutive runs. The root cause was that `resolveDependencies` pushed onto its `pkgAddresses` / `postponedResolutionsQueue` arrays from inside `Promise.all`-spawned callbacks, so completion-order timing leaked into the array order and downstream cyclic-peer suffix assignment. Fixes [#8155](https://github.com/pnpm/pnpm/issues/8155).
