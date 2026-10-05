## 1102.2.7

### Patch Changes

- `WantedDependency.alias` is now optional. `getWantedDependencies` returns `ManifestWantedDependency`, whose `alias` is required. Use `hasAlias` to narrow a `WantedDependency` to it.

- `pnpm add` and `pnpm install` now keep the peer dependencies that `pnpm-lock.yaml` records for a package they did not update. A registry whose metadata disagrees with the package's `package.json`, for example by omitting `peerDependenciesMeta`, made `pnpm add` and `pnpm dedupe` write different lockfiles, so `pnpm dedupe --check` failed after `pnpm add` [#16615](https://github.com/pnpm/pnpm/issues/16615).

- When a dependency moves an exact dependency of its own to an older version, a peer dependency that pnpm installed automatically now moves with it. Before, `pnpm install` and `pnpm dedupe` kept the newer locked version of the peer, so the lockfile held two copies of it, for example two copies of `vue` [pnpm/tasks#61](https://github.com/pnpm/tasks/issues/61).

- Updated dependencies:
  - @pnpm/catalogs.resolver@1100.1.2
  - @pnpm/config.normalize-registries@1101.0.3
  - @pnpm/config.version-policy@1100.2.7
  - @pnpm/constants@1102.0.1
  - @pnpm/deps.graph-hasher@1100.3.7
  - @pnpm/deps.path@1101.0.6
  - @pnpm/error@1100.2.2
  - @pnpm/fetching.pick-fetcher@1100.1.14
  - @pnpm/fs.graceful-fs@1100.2.5
  - @pnpm/fs.symlink-dependency@1100.0.24
  - @pnpm/lockfile.preferred-versions@1100.0.38
  - @pnpm/lockfile.pruner@1100.0.27
  - @pnpm/lockfile.utils@1102.1.6
  - @pnpm/patching.config@1100.1.9
  - @pnpm/pkg-manifest.reader@1100.0.22
  - @pnpm/pkg-manifest.utils@1100.4.9
  - @pnpm/resolving.npm-resolver@1104.2.5
