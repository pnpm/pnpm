## 1101.2.9

### Patch Changes

- `pnpm deploy` in a workspace with `injectWorkspacePackages` no longer crashes when a linked workspace package declares a peer dependency named after an `Object.prototype` member, such as `constructor`.

- `pnpm deploy` without `injectWorkspacePackages` now binds a peer dependency of a linked workspace package to the version its parent provides, as an injected install does. Previously the deploy failed with `ERR_PNPM_DEPLOY_AMBIGUOUS_PEER` whenever other packages in the deployed graph depended on different versions of that peer [#16807](https://github.com/pnpm/pnpm/issues/16807).

- When the registry rejects `pnpm stage publish`, the error message now starts with "Failed to stage package".

- Updated dependencies:
  - @pnpm/config.reader@1102.3.5
  - @pnpm/engine.runtime.commands@1101.1.9
  - @pnpm/engine.runtime.node-resolver@1101.3.8
  - @pnpm/exec.lifecycle@1100.1.24
  - @pnpm/fetching.directory-fetcher@1100.0.39
  - @pnpm/fs.indexed-pkg-importer@1100.0.34
  - @pnpm/installing.client@1100.3.16
  - @pnpm/installing.commands@1101.4.6
  - @pnpm/resolving.npm-resolver@1104.2.6
  - @pnpm/workspace.projects-filter@1100.0.49
  - @pnpm/workspace.projects-graph@1100.0.44
  - @pnpm/workspace.workspace-manifest-writer@1100.2.7
