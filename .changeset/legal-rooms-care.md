---
"@pnpm/resolve-dependencies": patch
pnpm: patch
---

Fix usages of the [`catalog:` protocol](https://pnpm.io/catalogs) in [injected local workspace packages](https://pnpm.io/package_json#dependenciesmetainjected). This previously errored with `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`. [#8715](https://github.com/pnpm/pnpm/issues/8715)
