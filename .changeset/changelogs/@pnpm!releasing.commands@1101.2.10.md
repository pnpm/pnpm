## 1101.2.10

### Patch Changes

- `pnpm deploy` now records the peer dependencies of the workspace packages it copies into the deployed lockfile. Before, pnpm rejected that lockfile with `--frozen-lockfile` when a deployed workspace package declared a peer and `injectWorkspacePackages` was off [#17093](https://github.com/pnpm/pnpm/issues/17093).

- Fixed `pnpm install --frozen-lockfile` and `verifyDepsBeforeRun` failing inside a directory created by `pnpm deploy`. `pnpm deploy` now writes self-contained workspace and lockfile settings. `pnpm deploy` also prunes unused `patchedDependencies` and preserves workspace-level dependency resolutions on deployed directory dependencies.

- Trusted publishing with OIDC now works with registries whose URL has a path, such as `https://registry.example/npm/private/`. pnpm previously sent the token exchange request to the root of the registry host.

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.18
  - @pnpm/cli.utils@1101.0.33
  - @pnpm/config.pick-registry-for-package@1101.0.3
  - @pnpm/config.reader@1102.3.6
  - @pnpm/deps.path@1101.0.7
  - @pnpm/engine.runtime.commands@1101.1.10
  - @pnpm/engine.runtime.node-resolver@1101.3.9
  - @pnpm/exec.lifecycle@1100.1.25
  - @pnpm/exec.pnpm-cli-runner@1100.0.6
  - @pnpm/fetching.directory-fetcher@1100.0.40
  - @pnpm/fs.indexed-pkg-importer@1100.0.35
  - @pnpm/fs.packlist@1100.0.9
  - @pnpm/installing.client@1100.3.17
  - @pnpm/installing.commands@1101.4.7
  - @pnpm/installing.deps-resolver@1102.2.9
  - @pnpm/lockfile.fs@1100.2.14
  - @pnpm/lockfile.peer-edges@1100.0.3
  - @pnpm/lockfile.types@1100.1.5
  - @pnpm/network.auth-header@1101.1.17
  - @pnpm/network.fetch@1100.1.23
  - @pnpm/releasing.exportable-manifest@1100.3.8
  - @pnpm/releasing.versioning@1100.3.7
  - @pnpm/resolving.npm-resolver@1104.2.7
  - @pnpm/resolving.registry.types@1100.2.4
  - @pnpm/resolving.resolver-base@1101.3.3
  - @pnpm/types@1102.1.2
  - @pnpm/workspace.projects-filter@1100.0.50
  - @pnpm/workspace.projects-graph@1100.0.45
  - @pnpm/workspace.projects-sorter@1101.0.2
  - @pnpm/workspace.task-scheduler@1100.0.5
  - @pnpm/workspace.workspace-manifest-writer@1100.2.8
