## 1100.1.24

### Patch Changes

- `pnpm add` and `pnpm install` now support installing bzip2 compressed tarballs [https://github.com/pnpm/pnpm/issues/6761](https://github.com/pnpm/pnpm/issues/6761).

- A repeat `pnpm install` in a workspace with a custom `modulesDir` now takes the up-to-date fast path. Before, pnpm looked for each workspace project's dependencies in `node_modules` and ran a full install every time.

- `pnpm install --ignore-pnpmfile` no longer removes `pnpmfileChecksum` from an up-to-date `pnpm-lock.yaml`. `pnpm install --frozen-lockfile --ignore-pnpmfile` no longer fails with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` when the lockfile records a `pnpmfileChecksum`. A command that resolves dependencies with the pnpmfile ignored still writes the lockfile without it [#10944](https://github.com/pnpm/pnpm/issues/10944).

- `pnpm install` and `pnpm run` now reinstall a single project that was moved or renamed together with its `node_modules`. Before, they reported "Already up to date" while links such as Windows junctions still pointed at the old location [#9512](https://github.com/pnpm/pnpm/issues/9512).

- `pnpm install` now relinks a direct dependency whose link in `node_modules` points to a missing target. Before, it reported "Already up to date" and left the broken link [#9758](https://github.com/pnpm/pnpm/issues/9758).

- pnpm no longer treats packages inside a custom `modulesDir` as workspace projects, including one that `packageConfigs` sets for a project. Before, with a `modulesDir` such as `vendor` and a `packages` pattern such as `**`, a repeat install ran the lifecycle scripts of dependencies that `allowBuilds` had not approved [#15412](https://github.com/pnpm/pnpm/pull/15412).

- Updated dependencies:
  - @pnpm/config.parse-overrides@1100.1.6
  - @pnpm/config.reader@1102.3.0
  - @pnpm/error@1100.2.0
  - @pnpm/installing.context@1101.0.6
  - @pnpm/lockfile.fs@1100.2.9
  - @pnpm/lockfile.settings-checker@1100.2.8
  - @pnpm/lockfile.verification@1100.1.7
  - @pnpm/resolving.resolver-base@1101.3.1
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.projects-reader@1101.1.0
  - @pnpm/workspace.state@1100.0.45
  - @pnpm/workspace.workspace-manifest-reader@1100.2.0
