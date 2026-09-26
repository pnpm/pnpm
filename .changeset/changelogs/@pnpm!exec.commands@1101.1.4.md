## 1101.1.4

### Patch Changes

- `pnpm exec` and `pnpm dlx` now set `npm_execpath`, `INIT_CWD`, `npm_node_execpath`, and `NODE` for child processes [#7037](https://github.com/pnpm/pnpm/issues/7037).

- `pnpm dlx` now keeps a separate cache entry for each Node.js major version. A package built under one Node.js major version, such as a native addon, is no longer reused under another [#8611](https://github.com/pnpm/pnpm/issues/8611).

- `pnpm exec` now sets the `PWD` environment variable to the directory the command runs in. Shells and tools that read `PWD` now report the logical path of a workspace package reached through a symlink [#1550](https://github.com/pnpm/pnpm/issues/1550).

- `pnpm --filter <project> <command>` and `pnpm -r <command>` now run a command installed in the selected projects' dependencies when none of them has a script by that name. This matches `pnpm <command>` in a single project. `pnpm run` with `--filter` or `-r` still reports the missing script [#10151](https://github.com/pnpm/pnpm/issues/10151).

- Fixed command lookup for custom `modulesDir` settings in `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install. Project `.hooks` scripts are read from the configured modules directory. Installs resolved by pnpr now preserve configured modules and executable directories. `pnpm bin` now reports the configured executable directory. In a workspace whose projects keep their own lockfiles, a `packageConfigs` entry that gives one project its own `modulesDir` is followed too [#3604](https://github.com/pnpm/pnpm/issues/3604).

- Tools installed in a custom `modulesDir` can load CommonJS plugins installed there, the same way they would from `node_modules`. When executables are symlinks, as with `preferSymlinkedExecutables` or the hoisted linker, this works for `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install. A symlinked tool started directly from a shell does not get it. Paths containing the platform path-list separator do not receive this fallback. `extendNodePath: false` disables this fallback [#3604](https://github.com/pnpm/pnpm/issues/3604).

- `pnpm run --recursive` no longer reports interrupted scripts as lifecycle failures after `Ctrl+C`.

- `pnpm restart` now runs the "stop" and "start" scripts when the package has no "restart" script. Previously it ran "stop" and then failed with "Missing script: restart" [#4750](https://github.com/pnpm/pnpm/issues/4750).

- `pnpm run` with `--loglevel` set to `warn`, `error`, or `silent` (or the same `loglevel` setting) no longer prints the `$ <command>` line before a script, nor the summary of the install that `verifyDepsBeforeRun` runs first. Both are info-level output [#8944](https://github.com/pnpm/pnpm/issues/8944).

- Fixed `pnpm install` failing with `EEXIST` when a concurrent install cleared the file or directory that was occupying a symlink path. On Windows, a symlink another process is still holding is no longer moved aside and recreated.

- Concurrent `pnpm run` and `pnpm exec` commands now serialize their dependency installs [#14551](https://github.com/pnpm/pnpm/issues/14551).

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.17
  - @pnpm/building.commands@1101.2.4
  - @pnpm/cli.utils@1101.0.29
  - @pnpm/config.reader@1102.3.0
  - @pnpm/config.version-policy@1100.2.4
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/crypto.hash@1100.0.6
  - @pnpm/deps.status@1100.1.24
  - @pnpm/engine.runtime.commands@1101.1.4
  - @pnpm/engine.runtime.system-version@1100.0.12
  - @pnpm/error@1100.2.0
  - @pnpm/exec.lifecycle@1100.1.19
  - @pnpm/exec.npm-lifecycle@1100.0.1
  - @pnpm/exec.pnpm-cli-runner@1100.0.4
  - @pnpm/installing.client@1100.3.11
  - @pnpm/installing.commands@1101.4.0
  - @pnpm/pkg-manifest.reader@1100.0.20
  - @pnpm/store.path@1100.0.8
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.injected-deps-syncer@1100.0.39
  - @pnpm/workspace.project-manifest-reader@1100.1.0
  - @pnpm/workspace.projects-sorter@1101.0.1
  - @pnpm/workspace.task-scheduler@1100.0.2
