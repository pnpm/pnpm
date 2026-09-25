## 1100.1.19

### Patch Changes

- A dependency that ships a `binding.gyp` and sets `gypfile: false` no longer gets the `node-gyp rebuild` install script pnpm synthesizes for it. Such a dependency needs no `allowBuilds` entry and is no longer listed under "Ignored build scripts".

- Fixed command lookup for custom `modulesDir` settings in `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install. Project `.hooks` scripts are read from the configured modules directory. Installs resolved by pnpr now preserve configured modules and executable directories. `pnpm bin` now reports the configured executable directory. In a workspace whose projects keep their own lockfiles, a `packageConfigs` entry that gives one project its own `modulesDir` is followed too [#3604](https://github.com/pnpm/pnpm/issues/3604).

- Tools installed in a custom `modulesDir` can load CommonJS plugins installed there, the same way they would from `node_modules`. When executables are symlinks, as with `preferSymlinkedExecutables` or the hoisted linker, this works for `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install. A symlinked tool started directly from a shell does not get it. Paths containing the platform path-list separator do not receive this fallback. `extendNodePath: false` disables this fallback [#3604](https://github.com/pnpm/pnpm/issues/3604).

- `pnpm run --recursive` no longer reports interrupted scripts as lifecycle failures after `Ctrl+C`.

- `pnpm remove` now runs the project's own `preuninstall`, `uninstall`, and `postuninstall` scripts. `preuninstall` and `uninstall` run before dependencies are unlinked. A failure in either stage aborts the removal. `postuninstall` runs after unlinking completes. The `ignoreScripts` setting and `--lockfile-only` skip all three stages [#3276](https://github.com/pnpm/pnpm/issues/3276).

- The root project's `preinstall` script now runs before dependencies are resolved and linked. A guard such as `npx only-allow yarn` can stop the install before pnpm populates `node_modules` [#3760](https://github.com/pnpm/pnpm/issues/3760).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.33
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/error@1100.2.0
  - @pnpm/exec.npm-lifecycle@1100.0.1
  - @pnpm/fetching.directory-fetcher@1100.0.35
  - @pnpm/pkg-manifest.reader@1100.0.20
  - @pnpm/store.controller-types@1101.3.1
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.task-scheduler@1100.0.2
