## 1100.3.21

### Patch Changes

- `pnpm install --frozen-lockfile` now succeeds when an optional dependency was unresolvable and skipped by the install that wrote the lockfile. Previously, frozen installs failed with `ERR_PNPM_OUTDATED_LOCKFILE`. The notice states that the dependency could not be resolved and names the requested range [pnpm/pnpm#3960](https://github.com/pnpm/pnpm/issues/3960).

- The error for an incompatible pnpm-lock.yaml now reports the lockfileVersion the file was generated with and the lockfileVersion the current pnpm supports. The error also warns that recreating the lockfile with `--force` may break the application and suggests installing the pnpm version that generated the lockfile [#848](https://github.com/pnpm/pnpm/issues/848).

- The ignored build scripts warning and the update notice are printed as plain lines when output is not a terminal, in CI, or with `--reporter append-only`. They were drawn inside a box that broke apart in CI logs [#9421](https://github.com/pnpm/pnpm/issues/9421).

- If a script is killed by a signal that pnpm survives, such as SIGPIPE, the error now names the signal: `Command failed with signal SIGPIPE.` [#9821](https://github.com/pnpm/pnpm/issues/9821).

- `pnpm store status` now lists only the modified packages when packages in the store were mutated. It previously also suggested running `pnpm install --force` to refetch them [#919](https://github.com/pnpm/pnpm/issues/919).

- Updated dependencies:
  - @pnpm/cli.meta@1100.1.1
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/deps.inspection.peers-issues-renderer@1100.0.15
  - @pnpm/error@1100.2.0
  - @pnpm/types@1102.1.1
