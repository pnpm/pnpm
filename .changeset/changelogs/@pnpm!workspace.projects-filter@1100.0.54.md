## 1100.0.54

### Patch Changes

- Relative directory selectors set on `config.filter` or `config.filterProd` by an `updateConfig` hook now resolve from the workspace root even when `--dir` is passed or `pnpm` runs from a subdirectory [#17906](https://github.com/pnpm/pnpm/issues/17906).

- A workspace project that downloads its own Node.js through `devEngines.runtime` now has its `engines.node` checked against that Node.js. Before, pnpm checked it against the shared Node.js version, so `engineStrict` could reject a project whose scripts run on a compatible runtime [pnpm/tasks#300](https://github.com/pnpm/tasks/issues/300).

- Updated dependencies:
  - @pnpm/workspace.projects-graph@1100.0.48
  - @pnpm/workspace.projects-reader@1101.1.7
