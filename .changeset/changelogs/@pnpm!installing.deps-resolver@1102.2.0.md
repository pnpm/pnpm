## 1102.2.0

### Minor Changes

- `catalogMode` and `--save-catalog` no longer move a local path, tarball, or `workspace:<path>` specifier into a catalog. Such a specifier is resolved against the project that declares it, so one catalog entry cannot mean the same directory for every project that references it [#14437](https://github.com/pnpm/pnpm/issues/14437).

### Patch Changes

- Installation no longer fails with `Cannot convert undefined or null to object` when a linked local dependency provides a peer dependency that is also provided by one of its ancestors. This was reachable via `pnpm deploy --legacy`.

- An auto-installed optional peer is now resolved to a version its declared peer range accepts, even when the workspace root depends on that package at a version outside the range. Previously the root's version was used and then reported as an unmet optional peer [#13867](https://github.com/pnpm/pnpm/issues/13867).

- `pnpm install` now relinks workspace packages when `publishConfig.linkDirectory` changes. Frozen installs report an outdated lockfile until it is regenerated [pnpm/pnpm#14488](https://github.com/pnpm/pnpm/issues/14488).

- Updated dependencies:
  - @pnpm/catalogs.resolver@1100.1.0
  - @pnpm/config.version-policy@1100.2.3
  - @pnpm/deps.graph-hasher@1100.3.1
  - @pnpm/error@1100.1.4
  - @pnpm/fetching.pick-fetcher@1100.1.10
  - @pnpm/hooks.types@1101.0.2
  - @pnpm/lockfile.preferred-versions@1100.0.32
  - @pnpm/lockfile.pruner@1100.0.22
  - @pnpm/lockfile.types@1100.1.1
  - @pnpm/lockfile.utils@1102.1.1
  - @pnpm/patching.config@1100.1.4
  - @pnpm/pkg-manifest.reader@1100.0.19
  - @pnpm/pkg-manifest.utils@1100.4.3
  - @pnpm/resolving.npm-resolver@1104.1.1
