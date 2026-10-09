## 12.11.2

### Patch Changes

- `enable-modules-dir=false` also fetches the packages an install reuses from an existing lockfile, not only the ones it resolves anew, so the store holds every package the lockfile lists, as with pnpm v10.
