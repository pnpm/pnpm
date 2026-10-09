## 12.11.2

This release fixes `--workspace-concurrency=Infinity`, filters set by an `updateConfig` hook, and store fetches with `enable-modules-dir=false`.

### Patch Changes

- `--workspace-concurrency=Infinity` now runs workspace projects with no concurrency limit. It used to fail with `invalid digit found in string` [#16793](https://github.com/pnpm/pnpm/issues/16793).

- `pnpm install` and other recursive commands now apply the `filter` and `filterProd` that an `updateConfig` hook sets. They used to run on every workspace project [#16792](https://github.com/pnpm/pnpm/issues/16792).

- `enable-modules-dir=false` now also fetches the packages an install reuses from an existing lockfile, so the store holds every package the lockfile lists.
