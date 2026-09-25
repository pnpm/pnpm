## 1100.0.30

### Patch Changes

- `pnpm deploy` now respects `--package-import-method` passed on the command line and reports the package import method correctly [pnpm/pnpm#7593](https://github.com/pnpm/pnpm/issues/7593).

- `pnpm install` in WSL now waits out Windows file locks on a Windows drive such as `/mnt/c`, as it already does on Windows. Before, an antivirus or indexer scan holding a file open could fail the install with `EACCES` [pnpm/pnpm#6155](https://github.com/pnpm/pnpm/issues/6155).

- `pnpm pack`, `pnpm deploy`, and installs of local directory dependencies now keep symlinks that point to files or directories included in the package. `pnpm pack` leaves out symlinks that point outside the package [#8208](https://github.com/pnpm/pnpm/issues/8208).

- Updated dependencies:
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/fs.graceful-fs@1100.2.3
  - @pnpm/store.controller-types@1101.3.1
