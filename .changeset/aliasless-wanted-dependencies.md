---
"@pnpm/catalogs.resolver": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.deps-installer": patch
---

`WantedDependency.alias` is now optional. `getWantedDependencies` returns `ManifestWantedDependency`, whose `alias` is required. Use `hasAlias` to narrow a `WantedDependency` to it.
