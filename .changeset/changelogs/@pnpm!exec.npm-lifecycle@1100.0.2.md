## 1100.0.2

### Patch Changes

- On Windows, `pnpm run` now passes the arguments after the script name to the script as typed. Before, `cmd` expanded `%VAR%` in them and backslashes arrived doubled. Line breaks still arrive as the two characters `\n`, because `cmd` cannot pass them. The command line pnpm prints for the script quotes the arguments the same way on every platform [#16257](https://github.com/pnpm/pnpm/issues/16257).

- `pnpm run` exits with the code of a script that handles Ctrl+C and shuts down. A script that finished cleanly is not reported as a lifecycle failure. The commands after it in the same script still run [pnpm/pnpm#9945](https://github.com/pnpm/pnpm/issues/9945).

- `pnpm run` and lifecycle scripts use the configured `scriptShell`, including Git Bash on Windows, when `shellEmulator` is also enabled. `shellEmulator` still runs scripts when `scriptShell` is not set. Extra arguments passed to `pnpm run` are quoted for the shell that runs the script, so a Windows path stays intact [#14719](https://github.com/pnpm/pnpm/issues/14719).

- pnpm no longer hangs after a lifecycle script exits while a process it started in the background keeps the script's output open. pnpm stops reading that output one second after the script exits [#5730](https://github.com/pnpm/pnpm/issues/5730).

- Updated dependencies:
  - @pnpm/error@1100.2.1
