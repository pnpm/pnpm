## 12.11.2

### Patch Changes

- `enable-modules-dir=false` also fetches the packages an install reuses from an existing lockfile, not only the ones it resolves anew, so the store holds every package the lockfile lists, as with pnpm v10.

- Fixed a regression where `pnpm install` and other recursive commands ignored the `filter` and `filterProd` that an `updateConfig` hook sets [#16792](https://github.com/pnpm/pnpm/issues/16792).

- `--workspace-concurrency=Infinity` now runs workspace projects with no concurrency limit. It used to fail with `invalid digit found in string` [#16793](https://github.com/pnpm/pnpm/issues/16793).
