---
"pacquet": patch
---

`pnpm install --frozen-lockfile` no longer fails with `ERR_PNPM_OUTDATED_LOCKFILE` when an injected workspace package publishes from `publishConfig.directory` and the `package.json` in that directory lists other dependencies than the project's own. pnpm now checks the lockfile against the project's manifest, the one it resolved the dependencies from.
