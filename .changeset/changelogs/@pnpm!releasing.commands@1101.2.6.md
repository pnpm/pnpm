## 1101.2.6

### Patch Changes

- `pnpm deploy` no longer copies the workspace root's `packageManager` and `devEngines.packageManager` fields into the deployed `package.json` [#16403](https://github.com/pnpm/pnpm/issues/16403).

- `pnpm deploy` no longer fails with `ERR_PNPM_DEPLOY_AMBIGUOUS_PEER` in a workspace with `injectWorkspacePackages: true` when a workspace package lists its peer dependency as a dev dependency too [#16375](https://github.com/pnpm/pnpm/issues/16375).

- `pnpm pack-app` now accepts an entry file or output directory inside the project whose name starts with two dots, such as `..build/entry.cjs`. It used to fail with `ERR_PNPM_PACK_APP_ENTRY_OUTSIDE_PROJECT`.

- `pnpm publish` now includes bare `README` files and README files with Markdown extensions such as `readme.markdown` in registry metadata [#12704](https://github.com/pnpm/pnpm/issues/12704).

- Updated dependencies:
  - @pnpm/config.reader@1102.3.2
  - @pnpm/engine.runtime.commands@1101.1.6
  - @pnpm/engine.runtime.node-resolver@1101.3.5
  - @pnpm/exec.lifecycle@1100.1.21
  - @pnpm/installing.client@1100.3.13
  - @pnpm/installing.commands@1101.4.3
  - @pnpm/lockfile.fs@1100.2.11
  - @pnpm/network.fetch@1100.1.20
  - @pnpm/releasing.exportable-manifest@1100.3.5
  - @pnpm/resolving.npm-resolver@1104.2.3
  - @pnpm/workspace.projects-filter@1100.0.46
  - @pnpm/workspace.projects-graph@1100.0.41
  - @pnpm/workspace.workspace-manifest-writer@1100.2.4
