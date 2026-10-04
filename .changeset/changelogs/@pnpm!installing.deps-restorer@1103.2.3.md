## 1103.2.3

### Patch Changes

- `pnpm install` with `nodeLinker: hoisted` now refreshes directories supplied by custom fetchers when reinstalling.

- With `nodeLinker: hoisted`, a filtered install of a workspace project no longer fails with `ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY` after a filtered install of another project.

- With `nodeLinker: hoisted`, a filtered install now keeps the packages of the workspace projects an earlier install put in `node_modules`. This also covers the install that `pnpm --filter <selector> run` and `pnpm --filter <selector> exec` start before the command. Before, these installs removed every package that only the unselected projects needed [#16483](https://github.com/pnpm/pnpm/issues/16483).

- Fixed frozen installs replacing a hoisted dependency with a workspace package of the same name. A later `pnpm dedupe` then removed the hoisted link [#16485](https://github.com/pnpm/pnpm/issues/16485).

- `pnpm install` now prints a warning with the error when an optional dependency cannot be fetched and is skipped. The skipped package is no longer linked into `node_modules` as a broken symlink or listed among the added dependencies. The `pnpm:skipped-optional-dependency` log reports the skip with the `fetch_failure` reason [#16514](https://github.com/pnpm/pnpm/issues/16514).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.36
  - @pnpm/bins.remover@1100.0.27
  - @pnpm/building.during-install@1102.2.6
  - @pnpm/building.policy@1100.1.5
  - @pnpm/config.package-is-installable@1100.2.2
  - @pnpm/core-loggers@1101.0.2
  - @pnpm/deps.graph-builder@1101.1.3
  - @pnpm/deps.graph-hasher@1100.3.6
  - @pnpm/exec.lifecycle@1100.1.22
  - @pnpm/fs.symlink-dependency@1100.0.23
  - @pnpm/installing.linking.direct-dep-linker@1100.0.23
  - @pnpm/installing.linking.hoist@1100.0.36
  - @pnpm/installing.linking.modules-cleaner@1100.1.28
  - @pnpm/installing.package-requester@1102.2.3
  - @pnpm/lockfile.filtering@1100.2.11
  - @pnpm/lockfile.fs@1100.2.12
  - @pnpm/lockfile.to-pnp@1101.0.9
  - @pnpm/pnpr.client@3.1.2
  - @pnpm/store.controller-types@1101.3.3
  - @pnpm/workspace.project-manifest-reader@1100.1.2
