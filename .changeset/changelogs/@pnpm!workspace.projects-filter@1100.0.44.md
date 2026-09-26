## 1100.0.44

### Patch Changes

- A dependency declared with `catalog:` now counts as a workspace dependency when its catalog entry points at a workspace project, for example `workspace:*`. `pnpm -r run` runs that project first. `--filter <pkg>...` selects it [#15587](https://github.com/pnpm/pnpm/issues/15587).

- `pnpm --filter "[<since>]"` now selects workspace packages when dependency versions change in a catalog in `pnpm-workspace.yaml` [#8718](https://github.com/pnpm/pnpm/issues/8718).

- Directory filters such as `--filter=./packages/*` now select projects when the current directory was entered with a lowercase drive letter on Windows, like `c:\repo` [#5500](https://github.com/pnpm/pnpm/issues/5500).

- `--filter "[<since>]"` now selects projects that files were moved out of when git detects the move as a rename [pnpm/pnpm#15481](https://github.com/pnpm/pnpm/issues/15481).

- `--filter` now evaluates selectors in order, so later inclusion filters can re-include packages that an earlier exclusion filter excluded [pnpm/pnpm#9354](https://github.com/pnpm/pnpm/issues/9354).

- The `[<since>]` filter selector now compares against the commit where the current branch forked from `<since>`. Projects changed only by newer commits on `<since>` are no longer selected. Uncommitted changes are still included. In a shallow clone without that commit, pnpm compares against `<since>` directly, as before [#9907](https://github.com/pnpm/pnpm/issues/9907).

- pnpm no longer treats packages inside a custom `modulesDir` as workspace projects, including one that `packageConfigs` sets for a project. Before, with a `modulesDir` such as `vendor` and a `packages` pattern such as `**`, a repeat install ran the lifecycle scripts of dependencies that `allowBuilds` had not approved [#15412](https://github.com/pnpm/pnpm/pull/15412).

- Updated dependencies:
  - @pnpm/catalogs.config@1100.0.8
  - @pnpm/error@1100.2.0
  - @pnpm/workspace.projects-graph@1100.0.39
  - @pnpm/workspace.projects-reader@1101.1.0
  - @pnpm/workspace.workspace-manifest-reader@1100.2.0
