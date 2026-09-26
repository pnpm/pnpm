## 1101.4.0

### Minor Changes

- Added the `forceIgnoresPlatform` setting. When it is `false`, `pnpm install --force` skips optional dependencies whose `os`, `cpu` or `libc` do not match the host instead of installing all of them. The default stays `true` [#6133](https://github.com/pnpm/pnpm/issues/6133).

### Patch Changes

- A dependency declared with `catalog:` now counts as a workspace dependency when its catalog entry points at a workspace project, for example `workspace:*`. `pnpm -r run` runs that project first. `--filter <pkg>...` selects it [#15587](https://github.com/pnpm/pnpm/issues/15587).

- `pnpm remove` now accepts `--trust-lockfile` and `--no-trust-lockfile` to control supply-chain policy checks while removing a package [#14406](https://github.com/pnpm/pnpm/issues/14406).

- `pnpm add` now saves changes to `package.json` before running lifecycle scripts, so a postinstall script failure leaves the added dependency in `package.json` [#8627](https://github.com/pnpm/pnpm/issues/8627).

- `pnpm outdated` and `pnpm update` now apply `minimumReleaseAge` to GitHub Actions. `minimumReleaseAgeExclude` entries match action names such as `actions/checkout` [#13923](https://github.com/pnpm/pnpm/issues/13923).

- `pnpm import` now converts dependencies that use Yarn's `patch:` protocol. The dependency keeps the version it patches, and the patch file is added to `patchedDependencies` in `pnpm-workspace.yaml`. If the patch file is missing, pnpm prints a warning and imports the dependency without the patch [#10278](https://github.com/pnpm/pnpm/issues/10278).

- `pnpm import` in a workspace now keeps the versions pinned by the root `yarn.lock`, `package-lock.json`, or `npm-shrinkwrap.json` when another workspace project's range allows a newer version. Before, the root project got the newest version in its range [#4385](https://github.com/pnpm/pnpm/issues/4385).

- Allow external dependencies to be updated when running `pnpm update --interactive` with `--workspace`.

- `pnpm deploy --legacy` no longer rewrites `node_modules/.pnpm-workspace-state-v1.json` in the source workspace. The next `verifyDepsBeforeRun` check there reported the workspace as out of date [#15352](https://github.com/pnpm/pnpm/issues/15352).

- Fixed command lookup for custom `modulesDir` settings in `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install. Project `.hooks` scripts are read from the configured modules directory. Installs resolved by pnpr now preserve configured modules and executable directories. `pnpm bin` now reports the configured executable directory. In a workspace whose projects keep their own lockfiles, a `packageConfigs` entry that gives one project its own `modulesDir` is followed too [#3604](https://github.com/pnpm/pnpm/issues/3604).

- `pnpm install --silent` no longer fails when the install is delegated to pacquet. pnpm also stops passing `-s`, `--loglevel` and the other reporting flags to pacquet [#11936](https://github.com/pnpm/pnpm/issues/11936).

- `minimumReleaseAgeExcludePrune` and `trustPolicyExcludePrune` now work in workspaces with `shared-workspace-lockfile=false`. Once every project has been installed, pnpm drops an entry only if no project lockfile records it. Undecided `allowBuilds` entries are pruned the same way [#14612](https://github.com/pnpm/pnpm/issues/14612).

- `pnpm remove -r` now fails if any requested dependency is absent from all selected workspace projects. Validation respects `--save-prod`, `--save-dev`, and `--save-optional` and completes before modifying project manifests [#2319](https://github.com/pnpm/pnpm/issues/2319).

- `pnpm install` and `pnpm run` now reinstall a single project that was moved or renamed together with its `node_modules`. Before, they reported "Already up to date" while links such as Windows junctions still pointed at the old location [#9512](https://github.com/pnpm/pnpm/issues/9512).

- `pnpm install` now relinks a direct dependency whose link in `node_modules` points to a missing target. Before, it reported "Already up to date" and left the broken link [#9758](https://github.com/pnpm/pnpm/issues/9758).

- `pnpm remove --help` no longer shows a `[@<version>]` suffix in its usage line. The command accepts package names only [#7751](https://github.com/pnpm/pnpm/issues/7751).

- `pnpm install` no longer adds `allowBuilds` placeholder entries to `pnpm-workspace.yaml` when it runs in CI or without a terminal. Interactive installs still add them [#11574](https://github.com/pnpm/pnpm/issues/11574).

- Fixed `pnpm install` for workspace projects reached through a symlink, such as a `packages` directory that links to a folder outside the workspace. pnpm now installs their dependencies, and the links in their `node_modules` resolve [#1044](https://github.com/pnpm/pnpm/issues/1044).

- `pnpm unlink` now removes the `link:` dependency that `pnpm link <dir>` added to `package.json`. The linked package is removed from `node_modules` and the lockfile. A `link:` dependency to another directory is kept [#4219](https://github.com/pnpm/pnpm/issues/4219).

- Added a `--peer` flag to `pnpm update` to update ranges in `peerDependencies` [#8081](https://github.com/pnpm/pnpm/issues/8081).

- `pnpm update --prod` no longer installs devDependencies when run in a project installed with `--prod` [#8038](https://github.com/pnpm/pnpm/issues/8038).

- pnpm now warns when a workspace install covers a project that has its own `pnpm-workspace.yaml`. The nested file's settings, such as `patchedDependencies`, do not apply when the outer workspace installs that project. pnpm reads settings only from the `pnpm-workspace.yaml` at the workspace root [#11724](https://github.com/pnpm/pnpm/issues/11724).

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.6
  - @pnpm/building.policy@1100.1.3
  - @pnpm/catalogs.config@1100.0.8
  - @pnpm/cli.utils@1101.0.29
  - @pnpm/config.pick-registry-for-package@1101.0.2
  - @pnpm/config.reader@1102.3.0
  - @pnpm/config.version-policy@1100.2.4
  - @pnpm/config.writer@1100.0.28
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/deps.github-actions@1100.1.11
  - @pnpm/deps.inspection.outdated@1100.1.32
  - @pnpm/deps.path@1101.0.4
  - @pnpm/deps.security.signatures@1102.0.5
  - @pnpm/deps.status@1100.1.24
  - @pnpm/error@1100.2.0
  - @pnpm/fs.graceful-fs@1100.2.3
  - @pnpm/global.commands@1102.0.3
  - @pnpm/global.packages@1101.1.4
  - @pnpm/hooks.pnpmfile@1100.0.33
  - @pnpm/installing.context@1101.0.6
  - @pnpm/installing.dedupe.check@1100.1.14
  - @pnpm/installing.deps-installer@1104.2.0
  - @pnpm/installing.env-installer@1103.0.6
  - @pnpm/lockfile.fs@1100.2.9
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/lockfile.utils@1102.1.4
  - @pnpm/network.auth-header@1101.1.14
  - @pnpm/network.fetch@1100.1.18
  - @pnpm/pkg-manifest.reader@1100.0.20
  - @pnpm/pkg-manifest.utils@1100.4.6
  - @pnpm/resolving.npm-resolver@1104.2.1
  - @pnpm/resolving.resolver-base@1101.3.1
  - @pnpm/store.connection-manager@1101.2.0
  - @pnpm/store.controller@1102.2.0
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.project-manifest-reader@1100.1.0
  - @pnpm/workspace.project-manifest-writer@1100.0.18
  - @pnpm/workspace.projects-filter@1100.0.44
  - @pnpm/workspace.projects-graph@1100.0.39
  - @pnpm/workspace.projects-reader@1101.1.0
  - @pnpm/workspace.projects-sorter@1101.0.1
  - @pnpm/workspace.root-finder@1100.1.0
  - @pnpm/workspace.state@1100.0.45
  - @pnpm/workspace.task-scheduler@1100.0.2
  - @pnpm/workspace.workspace-manifest-reader@1100.2.0
  - @pnpm/workspace.workspace-manifest-writer@1100.2.2
