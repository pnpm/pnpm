## 1101.2.11

### Patch Changes

- `pnpm -r publish --batch` now sends `PUT /-/pnpr/v0/publish`, the batch publish endpoint that pnpr serves.

- `pnpm deploy` now keeps a deployed symlink to a directory inside the project even if the directory is not among the deployed files, such as a link into `node_modules` [#17288](https://github.com/pnpm/pnpm/issues/17288).

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.19
  - @pnpm/cli.utils@1101.0.34
  - @pnpm/config.pick-registry-for-package@1101.0.4
  - @pnpm/config.reader@1102.3.7
  - @pnpm/deps.path@1101.0.8
  - @pnpm/engine.runtime.commands@1101.1.11
  - @pnpm/engine.runtime.node-resolver@1101.3.10
  - @pnpm/exec.lifecycle@1100.1.26
  - @pnpm/exec.pnpm-cli-runner@1100.0.7
  - @pnpm/fetching.directory-fetcher@1100.0.41
  - @pnpm/fs.indexed-pkg-importer@1100.0.36
  - @pnpm/fs.packlist@1100.0.10
  - @pnpm/installing.client@1100.3.18
  - @pnpm/installing.commands@1101.4.8
  - @pnpm/installing.deps-resolver@1102.2.10
  - @pnpm/lockfile.fs@1100.2.15
  - @pnpm/lockfile.peer-edges@1100.0.4
  - @pnpm/lockfile.types@1100.1.6
  - @pnpm/network.auth-header@1101.1.18
  - @pnpm/network.fetch@1100.1.24
  - @pnpm/releasing.exportable-manifest@1100.3.9
  - @pnpm/releasing.versioning@1100.3.8
  - @pnpm/resolving.npm-resolver@1104.2.8
  - @pnpm/resolving.registry.types@1100.2.5
  - @pnpm/resolving.resolver-base@1101.3.4
  - @pnpm/types@1102.1.3
  - @pnpm/workspace.projects-filter@1100.0.51
  - @pnpm/workspace.projects-graph@1100.0.46
  - @pnpm/workspace.projects-sorter@1101.0.3
  - @pnpm/workspace.task-scheduler@1100.0.6
  - @pnpm/workspace.workspace-manifest-writer@1100.2.9
