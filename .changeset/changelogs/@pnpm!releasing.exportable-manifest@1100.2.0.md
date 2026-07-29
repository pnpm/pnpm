## 1100.2.0

### Minor Changes

- Added support for `publishConfig.name`, which publishes a package under a different name than the one its manifest carries in the workspace. It is for a project whose published name is already taken by a sibling project, which otherwise has to be renamed by a build step just before publishing. Only the published artifact is renamed — dependents, `pnpm-lock.yaml`, and release tooling keep addressing the project by its manifest name — and the new name reaches the packed manifest, the tarball filename, and everything that addresses the package at the registry: the already-published check of `pnpm publish -r`, its registry selection, and the release-planning probes of `pnpm change status` and `pnpm version -r` [#13345](https://github.com/pnpm/pnpm/issues/13345).

### Patch Changes

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.12
  - @pnpm/types@1101.7.0
  - @pnpm/workspace.project-manifest-reader@1100.0.21
