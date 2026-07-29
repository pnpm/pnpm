## 1100.8.0

### Minor Changes

- Added support for `publishConfig.name`, which publishes a package under a different name than the one its manifest carries in the workspace. It is for a project whose published name is already taken by a sibling project, which otherwise has to be renamed by a build step just before publishing. Only the published artifact is renamed — dependents, `pnpm-lock.yaml`, and release tooling keep addressing the project by its manifest name — and the new name reaches the packed manifest, the tarball filename, and everything that addresses the package at the registry: the already-published check of `pnpm publish -r`, its registry selection, and the release-planning probes of `pnpm change status` and `pnpm version -r` [#13345](https://github.com/pnpm/pnpm/issues/13345).

### Patch Changes

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.12
  - @pnpm/cli.utils@1101.0.20
  - @pnpm/config.pick-registry-for-package@1100.0.13
  - @pnpm/config.reader@1101.15.0
  - @pnpm/deps.path@1100.0.12
  - @pnpm/engine.runtime.commands@1100.1.17
  - @pnpm/engine.runtime.node-resolver@1101.1.19
  - @pnpm/exec.lifecycle@1100.1.9
  - @pnpm/fetching.directory-fetcher@1100.0.26
  - @pnpm/fs.indexed-pkg-importer@1100.0.22
  - @pnpm/installing.client@1100.3.0
  - @pnpm/installing.commands@1100.12.1
  - @pnpm/lockfile.fs@1100.1.15
  - @pnpm/lockfile.types@1100.0.17
  - @pnpm/network.auth-header@1101.1.7
  - @pnpm/network.fetch@1100.1.9
  - @pnpm/releasing.exportable-manifest@1100.2.0
  - @pnpm/releasing.versioning@1100.2.2
  - @pnpm/resolving.npm-resolver@1102.1.9
  - @pnpm/resolving.registry.types@1100.1.7
  - @pnpm/resolving.resolver-base@1100.5.5
  - @pnpm/types@1101.7.0
  - @pnpm/workspace.projects-filter@1100.0.33
  - @pnpm/workspace.projects-sorter@1100.0.12
  - @pnpm/workspace.workspace-manifest-writer@1100.0.19
