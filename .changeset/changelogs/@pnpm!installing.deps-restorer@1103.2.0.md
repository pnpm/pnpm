## 1103.2.0

### Minor Changes

- Added the `forceIgnoresPlatform` setting. When it is `false`, `pnpm install --force` skips optional dependencies whose `os`, `cpu` or `libc` do not match the host instead of installing all of them. The default stays `true` [#6133](https://github.com/pnpm/pnpm/issues/6133).

### Patch Changes

- `pnpm install` and `pnpm add` no longer skip optional dependencies that the Node.js version resolved for a `devEngines.runtime` range supports, when the range uses `onFail: download`. An explicitly set `nodeVersion` still takes priority [#14628](https://github.com/pnpm/pnpm/issues/14628).

- `pnpm install` with `--filter` now installs only the dependencies of the selected projects when using `nodeLinker: hoisted` [#8882](https://github.com/pnpm/pnpm/issues/8882).

- With `nodeLinker: hoisted`, `hoistWorkspacePackages` now links each workspace project that `hoistPattern` or `publicHoistPattern` selects into the root `node_modules`, unless a hoisted package or a root dependency already uses its name. The project's bins are linked into the root `node_modules/.bin` [#7553](https://github.com/pnpm/pnpm/issues/7553).

- With `nodeLinker: hoisted`, `pnpm install` now removes the commands of the packages it removes from `node_modules/.bin`, such as a nested copy deduped into the root `node_modules` [#7568](https://github.com/pnpm/pnpm/issues/7568).

- A legacy `pnpm deploy` with `node-linker=hoisted` now puts the deployed project's direct dependencies at the top of the deployed `node_modules` [#9671](https://github.com/pnpm/pnpm/issues/9671).

- Dependencies and executable binaries are now correctly linked and accessible for workspace packages using `publishConfig.directory` and `publishConfig.linkDirectory` [pnpm/pnpm#8338](https://github.com/pnpm/pnpm/issues/8338).

- Tools installed in a custom `modulesDir` can load CommonJS plugins installed there, the same way they would from `node_modules`. When executables are symlinks, as with `preferSymlinkedExecutables` or the hoisted linker, this works for `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install. A symlinked tool started directly from a shell does not get it. Paths containing the platform path-list separator do not receive this fallback. `extendNodePath: false` disables this fallback [#3604](https://github.com/pnpm/pnpm/issues/3604).

- `pnpm install --prod`, `pnpm fetch --prod` and `pnpm deploy --prod` no longer install a devDependency that is only there to satisfy an optional peer dependency of a production dependency. `pnpm list`, `pnpm why`, `pnpm licenses`, `pnpm sbom` and `pnpm audit` leave it out of `--prod` results too. The same applies to `--dev`. A peer that is not optional is still installed and audited [#15344](https://github.com/pnpm/pnpm/issues/15344).

- `pnpm remove` now runs the project's own `preuninstall`, `uninstall`, and `postuninstall` scripts. `preuninstall` and `uninstall` run before dependencies are unlinked. A failure in either stage aborts the removal. `postuninstall` runs after unlinking completes. The `ignoreScripts` setting and `--lockfile-only` skip all three stages [#3276](https://github.com/pnpm/pnpm/issues/3276).

- The root project's `preinstall` script now runs before dependencies are resolved and linked. A guard such as `npx only-allow yarn` can stop the install before pnpm populates `node_modules` [#3760](https://github.com/pnpm/pnpm/issues/3760).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.33
  - @pnpm/bins.remover@1100.0.25
  - @pnpm/building.during-install@1102.2.3
  - @pnpm/building.policy@1100.1.3
  - @pnpm/config.normalize-registries@1101.0.2
  - @pnpm/config.package-is-installable@1100.2.0
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/deps.graph-builder@1101.1.0
  - @pnpm/deps.graph-hasher@1100.3.4
  - @pnpm/deps.path@1101.0.4
  - @pnpm/error@1100.2.0
  - @pnpm/exec.lifecycle@1100.1.19
  - @pnpm/fs.symlink-dependency@1100.0.21
  - @pnpm/installing.linking.direct-dep-linker@1100.0.21
  - @pnpm/installing.linking.hoist@1100.0.33
  - @pnpm/installing.linking.modules-cleaner@1100.1.25
  - @pnpm/installing.linking.real-hoist@1100.1.20
  - @pnpm/installing.modules-yaml@1101.0.4
  - @pnpm/installing.package-requester@1102.2.0
  - @pnpm/lockfile.filtering@1100.2.9
  - @pnpm/lockfile.fs@1100.2.9
  - @pnpm/lockfile.to-pnp@1101.0.6
  - @pnpm/lockfile.utils@1102.1.4
  - @pnpm/patching.config@1100.1.7
  - @pnpm/pkg-manifest.reader@1100.0.20
  - @pnpm/pnpr.client@3.0.4
  - @pnpm/store.controller-types@1101.3.1
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.project-manifest-reader@1100.1.0
