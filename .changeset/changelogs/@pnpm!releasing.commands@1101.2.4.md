## 1101.2.4

### Patch Changes

- `pnpm deploy` no longer creates extra directories inside the deploy target and workspace projects when using a relative deploy path [pnpm/pnpm#10981](https://github.com/pnpm/pnpm/issues/10981).

- Fixed `pnpm deploy --legacy` leaving broken links to nested local dependencies of workspace packages [#9575](https://github.com/pnpm/pnpm/issues/9575).

- `pnpm deploy` with a shared lockfile now copies workspace dependencies into the deploy directory, even when `packageImportMethod` is set to `hardlink`. Previously, their files were hard-linked to the workspace sources, so editing a source file also changed the deployed copy [pnpm/pnpm#12176](https://github.com/pnpm/pnpm/issues/12176).

- `pnpm install` no longer fails when a package from the registry declares a `file:` dependency on a directory inside itself, such as `"@types/css-tree": "file:./typings/css-tree"`. pnpm links that dependency to the directory inside the package, as npm and Yarn do. The lockfile records it as `link:<root>/typings/css-tree` [#9141](https://github.com/pnpm/pnpm/issues/9141).

- Fixed `pnpm deploy --prod` failing with `ERR_PNPM_OUTDATED_LOCKFILE` when the deployed project declares a `devEngines.runtime` with `onFail: download`. The runtime stays out of the deployed `node_modules` with the rest of the dev dependencies [#15703](https://github.com/pnpm/pnpm/issues/15703).

- `pnpm publish` now waits at least 5 minutes for the registry to answer a publish request, like npm. This fixes "409 Conflict - Failed to save packument" errors when the registry is slow to answer [pnpm/pnpm#11454](https://github.com/pnpm/pnpm/issues/11454).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.30
  - @pnpm/config.reader@1102.3.1
  - @pnpm/deps.path@1101.0.5
  - @pnpm/engine.runtime.commands@1101.1.5
  - @pnpm/engine.runtime.node-resolver@1101.3.4
  - @pnpm/error@1100.2.1
  - @pnpm/exec.lifecycle@1100.1.20
  - @pnpm/fetching.directory-fetcher@1100.0.36
  - @pnpm/fs.indexed-pkg-importer@1100.0.31
  - @pnpm/fs.packlist@1100.0.6
  - @pnpm/installing.client@1100.3.12
  - @pnpm/installing.commands@1101.4.1
  - @pnpm/lockfile.fs@1100.2.10
  - @pnpm/lockfile.peer-edges@1100.0.1
  - @pnpm/lockfile.types@1100.1.4
  - @pnpm/network.auth-header@1101.1.15
  - @pnpm/network.fetch@1100.1.19
  - @pnpm/network.git-utils@1100.0.6
  - @pnpm/network.web-auth@1101.6.2
  - @pnpm/releasing.exportable-manifest@1100.3.4
  - @pnpm/releasing.versioning@1100.3.4
  - @pnpm/resolving.npm-resolver@1104.2.2
  - @pnpm/resolving.registry.types@1100.2.2
  - @pnpm/resolving.resolver-base@1101.3.2
  - @pnpm/workspace.project-manifest-reader@1100.1.1
  - @pnpm/workspace.projects-filter@1100.0.45
  - @pnpm/workspace.projects-graph@1100.0.40
  - @pnpm/workspace.task-scheduler@1100.0.3
  - @pnpm/workspace.workspace-manifest-writer@1100.2.3
