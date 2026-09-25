## 1101.1.0

### Minor Changes

- Added `findWorkspaceDirSync`, `findPackagesSync`, `findWorkspaceProjectsSync`, `findWorkspaceProjectsNoCheckSync`, `readWorkspaceManifestSync`, and `readExactProjectManifestSync`.

### Patch Changes

- pnpm no longer treats packages inside a custom `modulesDir` as workspace projects, including one that `packageConfigs` sets for a project. Before, with a `modulesDir` such as `vendor` and a `packages` pattern such as `**`, a repeat install ran the lifecycle scripts of dependencies that `allowBuilds` had not approved [#15412](https://github.com/pnpm/pnpm/pull/15412).

- Workspace discovery now returns one project per directory when multiple manifest formats are present. It selects `package.json`, then `package.json5`, then `package.yaml` [pnpm/pnpm#3027](https://github.com/pnpm/pnpm/issues/3027).

- Wildcards in negated `packages` patterns of `pnpm-workspace.yaml` now match directories whose names start with a dot. For example, `!packages/**` now also excludes `packages/.dev/tool` when another pattern includes `.dev` explicitly.

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.29
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.package-patterns@1100.0.1
  - @pnpm/workspace.project-manifest-reader@1100.1.0
