## 1102.2.14

### Patch Changes

- With `autoInstallPeers`, an optional peer is no longer bound to a version outside its range that only another dependency's subtree brings in. Before, adding an unrelated dependency could flip that binding and rewrite the peer suffixes of other projects in the lockfile [#18162](https://github.com/pnpm/pnpm/issues/18162).

- `pnpm install` resolves the `link:` and bare relative path dependencies of an injected workspace package from that package's directory. They were resolved from the lockfile directory, so the installed link could point outside the workspace [pnpm/tasks#274](https://github.com/pnpm/tasks/issues/274).

- Frozen installs accept injected workspace packages whose child dependencies use `link:` or a bare relative path [pnpm/tasks#274](https://github.com/pnpm/tasks/issues/274).

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.6
  - @pnpm/config.pick-registry-for-package@1101.0.6
  - @pnpm/config.version-policy@1100.2.11
  - @pnpm/core-loggers@1101.0.5
  - @pnpm/deps.graph-hasher@1100.3.13
  - @pnpm/deps.path@1101.0.10
  - @pnpm/fetching.pick-fetcher@1100.1.18
  - @pnpm/fs.symlink-dependency@1100.0.27
  - @pnpm/hooks.types@1101.0.9
  - @pnpm/lockfile.preferred-versions@1100.0.44
  - @pnpm/lockfile.pruner@1100.0.31
  - @pnpm/lockfile.types@1100.1.8
  - @pnpm/lockfile.utils@1102.1.12
  - @pnpm/patching.config@1100.1.13
  - @pnpm/pkg-manifest.reader@1100.0.25
  - @pnpm/pkg-manifest.utils@1100.4.13
  - @pnpm/resolving.local-resolver@1101.2.10
  - @pnpm/resolving.npm-resolver@1104.2.11
  - @pnpm/resolving.resolver-base@1101.3.6
  - @pnpm/store.controller-types@1101.3.7
  - @pnpm/types@1102.1.4
