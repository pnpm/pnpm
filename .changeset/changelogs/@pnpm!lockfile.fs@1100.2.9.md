## 1100.2.9

### Patch Changes

- `pnpm add` and `pnpm install` now support installing bzip2 compressed tarballs [https://github.com/pnpm/pnpm/issues/6761](https://github.com/pnpm/pnpm/issues/6761).

- Interrupting `pnpm install` with Ctrl+C or SIGTERM no longer leaves a temporary lockfile (`.pnpm-lock.yaml.*.tmp`) behind in the project [#1418](https://github.com/pnpm/pnpm/issues/1418).

- `pnpm install` now applies changes to or removal of a global `readPackage` hook when an existing lockfile is present [pnpm/pnpm#15136](https://github.com/pnpm/pnpm/issues/15136).

- The error for an incompatible pnpm-lock.yaml now reports the lockfileVersion the file was generated with and the lockfileVersion the current pnpm supports. The error also warns that recreating the lockfile with `--force` may break the application and suggests installing the pnpm version that generated the lockfile [#848](https://github.com/pnpm/pnpm/issues/848).

- On Windows, pnpm now retries saving `pnpm-lock.yaml` for up to a minute while another process holds the file open. The save used to fail at once with `EPERM`, `EBUSY`, or "Access is denied" [#9461](https://github.com/pnpm/pnpm/issues/9461).

- Updated dependencies:
  - @pnpm/deps.path@1101.0.4
  - @pnpm/error@1100.2.0
  - @pnpm/fs.graceful-fs@1100.2.3
  - @pnpm/lockfile.merger@1100.0.24
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/lockfile.utils@1102.1.4
  - @pnpm/network.git-utils@1100.0.5
  - @pnpm/types@1102.1.1
