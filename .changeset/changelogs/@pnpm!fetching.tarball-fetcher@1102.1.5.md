## 1102.1.5

### Patch Changes

- A tarball whose integrity pnpm computed during download is now found in the store on the next install. Before, that install downloaded the tarball again once the lockfile recorded the integrity [#12562](https://github.com/pnpm/pnpm/issues/12562).

- Local tarball dependencies using the file protocol are no longer counted as downloaded in the progress banner [pnpm/pnpm#1103](https://github.com/pnpm/pnpm/issues/1103).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/error@1100.2.1
  - @pnpm/exec.prepare-package@1100.0.39
  - @pnpm/fetching.fetcher-base@1100.2.12
  - @pnpm/fs.graceful-fs@1100.2.4
  - @pnpm/fs.packlist@1100.0.6
  - @pnpm/network.fetch@1100.1.19
  - @pnpm/store.index@1100.3.3
