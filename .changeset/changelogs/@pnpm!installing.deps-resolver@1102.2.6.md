## 1102.2.6

### Patch Changes

- `pnpm install --frozen-lockfile` no longer fails with `ERR_PNPM_OUTDATED_LOCKFILE` for a workspace project that declares `dependenciesMeta` and whose dependencies are all workspace links. pnpm now records that project's `dependenciesMeta` in `pnpm-lock.yaml` [#16457](https://github.com/pnpm/pnpm/issues/16457).

- Updated dependencies:
  - @pnpm/config.version-policy@1100.2.6
  - @pnpm/core-loggers@1101.0.2
  - @pnpm/deps.graph-hasher@1100.3.6
  - @pnpm/fs.symlink-dependency@1100.0.23
  - @pnpm/lockfile.preferred-versions@1100.0.37
  - @pnpm/pkg-manifest.utils@1100.4.8
  - @pnpm/resolving.npm-resolver@1104.2.4
  - @pnpm/store.controller-types@1101.3.3
