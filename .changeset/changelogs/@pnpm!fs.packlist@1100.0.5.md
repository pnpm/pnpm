## 1100.0.5

### Patch Changes

- `pnpm pack` now honors the `files` field of `package.yaml` and `package.json5` manifests. Git-hosted and injected local dependencies that use these manifests now honor it too [#7906](https://github.com/pnpm/pnpm/issues/7906).

- `pnpm pack`, `pnpm deploy`, and installs of local directory dependencies now keep symlinks that point to files or directories included in the package. `pnpm pack` leaves out symlinks that point outside the package [#8208](https://github.com/pnpm/pnpm/issues/8208).

- `pnpm pack` and `pnpm publish` now include bundled dependencies when using the isolated linker. This covers workspace packages and the dependencies of each bundled package. Bundled dependencies are also included when `publishConfig.directory` selects a build directory [pnpm/pnpm#1643](https://github.com/pnpm/pnpm/issues/1643).

- Updated dependencies:
  - @pnpm/workspace.project-manifest-reader@1100.1.0
