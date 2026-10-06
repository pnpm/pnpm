## 1100.1.2

### Patch Changes

- `WantedDependency.alias` is now optional. `getWantedDependencies` returns `ManifestWantedDependency`, whose `alias` is required. Use `hasAlias` to narrow a `WantedDependency` to it.
