## 1100.0.42

### Patch Changes

- Fixed the `make-dedicated-lockfile` command failing to start with `ReferenceError: require is not defined in ES module scope`.

- `make-dedicated-lockfile` now restores `package.json` when it cannot move the original `node_modules` back to its place. The error then names `.tmp_node_modules`, where the original `node_modules` was left. The command refuses to run while that directory exists, so a retry cannot overwrite it.

- `make-dedicated-lockfile` no longer removes fields such as `main` and `types` from the `publishConfig` of the project's `package.json`.

- `make-dedicated-lockfile` keeps workspace dependencies linked in the dedicated lockfile [#3442](https://github.com/pnpm/pnpm/issues/3442).

- Updated dependencies:
  - @pnpm/error@1100.2.0
  - @pnpm/lockfile.fs@1100.2.9
  - @pnpm/lockfile.pruner@1100.0.25
  - @pnpm/releasing.exportable-manifest@1100.3.3
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.project-manifest-reader@1100.1.0
  - @pnpm/workspace.root-finder@1100.1.0
