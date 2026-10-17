---
"pacquet": patch
---

`pnpm install --frozen-lockfile` now accepts the lockfile it wrote for an injected workspace package that publishes from `publishConfig.directory`, when the `package.json` in that directory lists other dependencies than the project's own. Before, it failed with `ERR_PNPM_OUTDATED_LOCKFILE`.
