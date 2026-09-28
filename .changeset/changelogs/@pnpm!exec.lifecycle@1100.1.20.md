## 1100.1.20

### Patch Changes

- On Windows, `pnpm run` now passes the arguments after the script name to the script as typed. Before, `cmd` expanded `%VAR%` in them and backslashes arrived doubled. Line breaks still arrive as the two characters `\n`, because `cmd` cannot pass them. The command line pnpm prints for the script quotes the arguments the same way on every platform [#16257](https://github.com/pnpm/pnpm/issues/16257).

- `pnpm install` no longer fails for an injected workspace dependency whose package publishes from a `publishConfig.directory` that its own `prepare` script builds. The injected copy now picks up that directory once `prepare` finishes building it. `pnpm install --frozen-lockfile` no longer reports the dependency as outdated while the directory has not been built yet. [pnpm/pnpm#7811](https://github.com/pnpm/pnpm/issues/7811)

- `pnpm run` exits with the code of a script that handles Ctrl+C and shuts down. A script that finished cleanly is not reported as a lifecycle failure. The commands after it in the same script still run [pnpm/pnpm#9945](https://github.com/pnpm/pnpm/issues/9945).

- `pnpm run` and lifecycle scripts use the configured `scriptShell`, including Git Bash on Windows, when `shellEmulator` is also enabled. `shellEmulator` still runs scripts when `scriptShell` is not set. Extra arguments passed to `pnpm run` are quoted for the shell that runs the script, so a Windows path stays intact [#14719](https://github.com/pnpm/pnpm/issues/14719).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.34
  - @pnpm/error@1100.2.1
  - @pnpm/exec.npm-lifecycle@1100.0.2
  - @pnpm/fetching.directory-fetcher@1100.0.36
  - @pnpm/pkg-manifest.reader@1100.0.21
  - @pnpm/store.cafs-types@1100.1.1
  - @pnpm/store.controller-types@1101.3.2
  - @pnpm/workspace.task-scheduler@1100.0.3
