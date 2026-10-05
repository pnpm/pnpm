## 1103.0.10

### Patch Changes

- pnpm now verifies locked config dependencies against their registry before installing them. Config dependencies must come from an npm registry. The lockfile can no longer replace the integrity of a config dependency pinned with `version+integrity`.

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.2.3
  - @pnpm/config.writer@1100.0.32
  - @pnpm/constants@1102.0.1
  - @pnpm/deps.graph-hasher@1100.3.7
  - @pnpm/deps.path@1101.0.6
  - @pnpm/error@1100.2.2
  - @pnpm/fs.read-modules-dir@1100.0.4
  - @pnpm/fs.symlink-dependency@1100.0.24
  - @pnpm/installing.deps-resolver@1102.2.7
  - @pnpm/lockfile.fs@1100.2.13
  - @pnpm/lockfile.pruner@1100.0.27
  - @pnpm/lockfile.utils@1102.1.6
  - @pnpm/network.auth-header@1101.1.16
  - @pnpm/network.fetch@1100.1.22
  - @pnpm/pkg-manifest.reader@1100.0.22
  - @pnpm/resolving.npm-resolver@1104.2.5
  - @pnpm/resolving.tarball-url@1101.1.4
  - @pnpm/store.controller@1102.2.4
