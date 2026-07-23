## 1100.7.0

### Minor Changes

- The first release of a package now publishes the version written in its manifest verbatim, instead of bumping off it. `pnpm version -r` and `pnpm change status` check the registry for each release's current version; when that version is not yet published, the package debuts at it and its pending changesets apply only from the next release. A newly added package seeded at `1100.0.0` with a `minor` changeset is therefore published as `1100.0.0` rather than skipping straight to `1100.1.0`.

### Patch Changes

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.10
  - @pnpm/cli.utils@1101.0.18
  - @pnpm/config.pick-registry-for-package@1100.0.11
  - @pnpm/config.reader@1101.13.0
  - @pnpm/deps.path@1100.0.10
  - @pnpm/engine.runtime.commands@1100.1.15
  - @pnpm/engine.runtime.node-resolver@1101.1.17
  - @pnpm/exec.lifecycle@1100.1.7
  - @pnpm/fetching.directory-fetcher@1100.0.24
  - @pnpm/fs.indexed-pkg-importer@1100.0.20
  - @pnpm/installing.client@1100.2.19
  - @pnpm/installing.commands@1100.11.0
  - @pnpm/lockfile.fs@1100.1.13
  - @pnpm/lockfile.types@1100.0.15
  - @pnpm/network.auth-header@1101.1.5
  - @pnpm/network.fetch@1100.1.7
  - @pnpm/releasing.exportable-manifest@1100.1.14
  - @pnpm/releasing.versioning@1100.2.0
  - @pnpm/resolving.npm-resolver@1102.1.7
  - @pnpm/resolving.registry.types@1100.1.5
  - @pnpm/resolving.resolver-base@1100.5.3
  - @pnpm/types@1101.5.0
  - @pnpm/workspace.projects-filter@1100.0.31
  - @pnpm/workspace.projects-sorter@1100.0.10
  - @pnpm/workspace.workspace-manifest-writer@1100.0.17
