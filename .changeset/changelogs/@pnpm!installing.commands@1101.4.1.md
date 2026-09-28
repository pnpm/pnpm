## 1101.4.1

### Patch Changes

- `pnpm add <dir>` now warns when the added directory declares peer dependencies, as `pnpm link` does. The directory is saved as a `link:` dependency, and its peers are not resolved from the project that adds it. Use the `file:` protocol to have them resolved [#5523](https://github.com/pnpm/pnpm/issues/5523).

- `pnpm install --dev` and `pnpm fetch --dev` now install the optional dependencies of devDependencies, such as the platform binaries of Biome and oxlint. The project's own `optionalDependencies` are still skipped [#9678](https://github.com/pnpm/pnpm/issues/9678).

- `pnpm install` now restores cached build artifacts when reinstalling a workspace that uses separate lockfiles [#12942](https://github.com/pnpm/pnpm/issues/12942).

- With `nodeLinker: hoisted`, `pnpm install` now restores a workspace project's `node_modules` after it was deleted. Before, the install printed "Already up to date" and left the project without the dependencies nested under it.

- `pnpm import` in a workspace now keeps the versions pinned by a `yarn.lock` inside a workspace project [#4385](https://github.com/pnpm/pnpm/issues/4385).

- `pnpm install` no longer fails for an injected workspace dependency whose package publishes from a `publishConfig.directory` that its own `prepare` script builds. The injected copy now picks up that directory once `prepare` finishes building it. `pnpm install --frozen-lockfile` no longer reports the dependency as outdated while the directory has not been built yet. [pnpm/pnpm#7811](https://github.com/pnpm/pnpm/issues/7811)

- With `sharedWorkspaceLockfile: false`, an injected workspace package that has lifecycle scripts is now hard linked into the projects that depend on it. Before, pnpm left a plain copy, so later edits to the package did not reach those projects [#9828](https://github.com/pnpm/pnpm/issues/9828).

- `pnpm update --global` now reinstalls the global packages that pnpm 10 installed into the previous global directory, `<global-dir>/5`, so their commands are linked into the pnpm home `bin` directory again and `pnpm list --global` lists them. Once every package is migrated, pnpm deletes the previous directory and the commands pnpm 10 linked into the pnpm home [#11528](https://github.com/pnpm/pnpm/issues/11528).

- `pnpm install` on CI now fails on an outdated lockfile when `preferFrozenLockfile` is explicitly set to `true`. Setting it to `true` used to let CI update the lockfile [#9072](https://github.com/pnpm/pnpm/pull/9072).

- With the default and append-only reporters, installs with `--loglevel warn` or `--loglevel error` now print the full output of a failed install script. The output of successful scripts, including the root project's own install hooks, stays hidden. With `--loglevel warn`, pnpm also prints ignored build script warnings.

- `pnpm install` now fails with `ERR_PNPM_IGNORED_BUILDS` on a repeat install when `strictDepBuilds` is on and a dependency's build is still undecided. A repeat install against an existing `node_modules` reported success where a fresh install failed [pnpm/pnpm#10450](https://github.com/pnpm/pnpm/issues/10450).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/building.after-install@1103.0.7
  - @pnpm/building.policy@1100.1.4
  - @pnpm/catalogs.config@1100.0.9
  - @pnpm/cli.utils@1101.0.30
  - @pnpm/config.reader@1102.3.1
  - @pnpm/config.version-policy@1100.2.5
  - @pnpm/config.writer@1100.0.29
  - @pnpm/deps.github-actions@1100.1.12
  - @pnpm/deps.inspection.outdated@1100.1.33
  - @pnpm/deps.path@1101.0.5
  - @pnpm/deps.security.signatures@1102.0.6
  - @pnpm/deps.status@1100.1.25
  - @pnpm/error@1100.2.1
  - @pnpm/fs.graceful-fs@1100.2.4
  - @pnpm/fs.read-modules-dir@1100.0.3
  - @pnpm/global.commands@1102.0.4
  - @pnpm/global.packages@1101.1.5
  - @pnpm/hooks.pnpmfile@1100.0.34
  - @pnpm/installing.context@1101.0.7
  - @pnpm/installing.dedupe.check@1100.1.15
  - @pnpm/installing.deps-installer@1104.2.1
  - @pnpm/installing.env-installer@1103.0.7
  - @pnpm/installing.modules-yaml@1101.0.5
  - @pnpm/lockfile.fs@1100.2.10
  - @pnpm/lockfile.types@1100.1.4
  - @pnpm/lockfile.utils@1102.1.5
  - @pnpm/network.auth-header@1101.1.15
  - @pnpm/network.fetch@1100.1.19
  - @pnpm/pkg-manifest.reader@1100.0.21
  - @pnpm/pkg-manifest.utils@1100.4.7
  - @pnpm/resolving.local-resolver@1101.2.4
  - @pnpm/resolving.npm-resolver@1104.2.2
  - @pnpm/resolving.parse-wanted-dependency@1100.0.3
  - @pnpm/resolving.resolver-base@1101.3.2
  - @pnpm/store.connection-manager@1101.3.0
  - @pnpm/store.controller@1102.2.1
  - @pnpm/workspace.injected-deps-syncer@1100.0.40
  - @pnpm/workspace.project-manifest-reader@1100.1.1
  - @pnpm/workspace.project-manifest-writer@1100.0.19
  - @pnpm/workspace.projects-filter@1100.0.45
  - @pnpm/workspace.projects-graph@1100.0.40
  - @pnpm/workspace.projects-reader@1101.1.1
  - @pnpm/workspace.root-finder@1100.1.1
  - @pnpm/workspace.state@1100.0.46
  - @pnpm/workspace.task-scheduler@1100.0.3
  - @pnpm/workspace.workspace-manifest-reader@1100.2.1
  - @pnpm/workspace.workspace-manifest-writer@1100.2.3
