## 1100.0.4

### Patch Changes

- Commands that run pnpm again, such as `pnpm runtime set` and `pnpm env use`, no longer re-run a script that only looks like pnpm. A script named `pnpm` or `pn` that another package installed was run as though it were pnpm.

- Scripts that `pnpx` and `pnx` run now get pnpm itself as `npm_execpath`. A script that ran `$npm_execpath install` there ran `pnpm dlx install`.

- `pnpm run` with `--loglevel` set to `warn`, `error`, or `silent` (or the same `loglevel` setting) no longer prints the `$ <command>` line before a script, nor the summary of the install that `verifyDepsBeforeRun` runs first. Both are info-level output [#8944](https://github.com/pnpm/pnpm/issues/8944).

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.1
