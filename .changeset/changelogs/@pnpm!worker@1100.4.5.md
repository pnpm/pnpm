## 1100.4.5

### Patch Changes

- `pnpm install` now completes after downloading a Node.js runtime specified by `devEngines.runtime` when pnpm runs on Node.js 24.4.x. [#14667](https://github.com/pnpm/pnpm/issues/14667).

- A tarball whose integrity pnpm computed during download is now found in the store on the next install. Before, that install downloaded the tarball again once the lockfile recorded the integrity [#12562](https://github.com/pnpm/pnpm/issues/12562).

- The side-effects cache now restores the symlinks that a build script creates inside a package. A warm install used to replace each of them with a copy of its target [#12859](https://github.com/pnpm/pnpm/issues/12859).

  After upgrading, every package with a build script is built once more.

- pnpm now uses less memory when installing a package whose archive is larger than 64 MiB unpacked. It decompresses such archives as a stream [#14164](https://github.com/pnpm/pnpm/issues/14164).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- On Windows, `pnpm install` no longer skips a dependency's build script on a later install when the script changes nothing inside the package directory [#15667](https://github.com/pnpm/pnpm/issues/15667).

- Updated dependencies:
  - @pnpm/building.pkg-requires-build@1100.0.19
  - @pnpm/crypto.integrity@1100.0.8
  - @pnpm/error@1100.2.1
  - @pnpm/fs.graceful-fs@1100.2.4
  - @pnpm/fs.hard-link-dir@1100.0.8
  - @pnpm/fs.symlink-dependency@1100.0.22
  - @pnpm/store.cafs@1100.3.5
  - @pnpm/store.cafs-types@1100.1.1
  - @pnpm/store.create-cafs-store@1100.0.32
  - @pnpm/store.index@1100.3.3
