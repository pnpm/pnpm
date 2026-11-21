---
"pacquet": patch
---

`pnpm install` with `pnprServer` now applies `injectWorkspacePackages`, `dedupeInjectedDeps`, `ignoredOptionalDependencies`, and `dependenciesMeta`. Projects that use them no longer fail with `ERR_PNPM_OUTDATED_LOCKFILE` or `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH`.

`pnpm install --update-checksums` with `pnprServer` now refreshes the integrity checksums in the lockfile.

With `pnprServer` set and `blockExoticSubdeps: false`, pnpm resolves locally.
