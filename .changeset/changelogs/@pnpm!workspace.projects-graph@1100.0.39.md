## 1100.0.39

### Patch Changes

- A dependency declared with `catalog:` now counts as a workspace dependency when its catalog entry points at a workspace project, for example `workspace:*`. `pnpm -r run` runs that project first. `--filter <pkg>...` selects it [#15587](https://github.com/pnpm/pnpm/issues/15587).

- With `linkWorkspacePackages` enabled, a dependency declared as an `npm:` alias of a workspace project, such as `"math-alias": "npm:math@^1.0.0"`, now counts as a workspace dependency. `pnpm -r run` runs that project first. `--filter <pkg>...` selects it.

- Updated dependencies:
  - @pnpm/resolving.npm-resolver@1104.2.1
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.range-resolver@1100.0.4
