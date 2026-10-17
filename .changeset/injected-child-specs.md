---
"pacquet": patch
---

`pnpm install --frozen-lockfile` now accepts the lockfile it wrote for an injected workspace package that bundles a dependency (`bundledDependencies`), or that lists a dependency in both `dependencies` and `optionalDependencies`. Before, it failed with `ERR_PNPM_OUTDATED_LOCKFILE`.
