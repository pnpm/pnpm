## 1100.0.1

### Patch Changes

- `pnpm exec` and `pnpm dlx` now set `npm_execpath`, `INIT_CWD`, `npm_node_execpath`, and `NODE` for child processes [#7037](https://github.com/pnpm/pnpm/issues/7037).

- A script that runs `pnpm run` no longer adds duplicate `node_modules/.bin` and `node-gyp-bin` entries to `PATH` [#5352](https://github.com/pnpm/pnpm/issues/5352).

- When the configured `scriptShell` does not exist, running a script now fails with an error that names the shell. Previously pnpm printed only an exit code or the package directory [#7562](https://github.com/pnpm/pnpm/issues/7562).

- Fixed command lookup for custom `modulesDir` settings in `pnpm run`, `pnpm exec`, `pnpm version` hooks, and the lifecycle scripts a project runs during install. Project `.hooks` scripts are read from the configured modules directory. Installs resolved by pnpr now preserve configured modules and executable directories. `pnpm bin` now reports the configured executable directory. In a workspace whose projects keep their own lockfiles, a `packageConfigs` entry that gives one project its own `modulesDir` is followed too [#3604](https://github.com/pnpm/pnpm/issues/3604).

- Scripts that `pnpx` and `pnx` run now get pnpm itself as `npm_execpath`. A script that ran `$npm_execpath install` there ran `pnpm dlx install`.

- `pnpm run --recursive` no longer reports interrupted scripts as lifecycle failures after `Ctrl+C`.

- A script that pnpm runs without a terminal now ends when pnpm itself is killed. Killing pnpm's process group, as Playwright's `webServer` does to stop the command it started, used to leave the script running and holding the caller's output pipes open [#15555](https://github.com/pnpm/pnpm/issues/15555).

- Fixed scripts failing with errors such as `'an-compile' is not recognized` when `scriptShell` is set to `cmd.exe` on Windows [#7181](https://github.com/pnpm/pnpm/issues/7181).

- If a script is killed by a signal that pnpm survives, such as SIGPIPE, the error now names the signal: `Command failed with signal SIGPIPE.` [#9821](https://github.com/pnpm/pnpm/issues/9821).

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.1
  - @pnpm/error@1100.2.0
