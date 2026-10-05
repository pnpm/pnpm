---
"@pnpm/catalogs.resolver": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.deps-installer": patch
---

`WantedDependency.alias` is now optional, because Git, JSR, and tarball selectors have no alias until they are resolved. `getWantedDependencies` returns `ManifestWantedDependency`, whose `alias` is required. Use `hasAlias` to narrow a `WantedDependency` to it.
