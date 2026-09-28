## 1101.0.7

### Patch Changes

- `pnpm patch-commit` now fails with an error when `git` cannot be found in `PATH`. It previously reported that no changes were found [pnpm/pnpm#8666](https://github.com/pnpm/pnpm/issues/8666).

- `pnpm patch` now applies the existing patch file to the edit directory of a git-hosted dependency, as it already does for packages from the registry [#9699](https://github.com/pnpm/pnpm/issues/9699).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.30
  - @pnpm/config.reader@1102.3.1
  - @pnpm/config.writer@1100.0.29
  - @pnpm/crypto.hash@1100.0.7
  - @pnpm/error@1100.2.1
  - @pnpm/fs.packlist@1100.0.6
  - @pnpm/installing.commands@1101.4.1
  - @pnpm/installing.modules-yaml@1101.0.5
  - @pnpm/lockfile.fs@1100.2.10
  - @pnpm/lockfile.pruner@1100.0.26
  - @pnpm/lockfile.utils@1102.1.5
  - @pnpm/patching.apply-patch@1100.0.10
  - @pnpm/pkg-manifest.reader@1100.0.21
  - @pnpm/resolving.parse-wanted-dependency@1100.0.3
  - @pnpm/store.connection-manager@1101.3.0
  - @pnpm/store.path@1100.1.0
  - @pnpm/workspace.project-manifest-reader@1100.1.1
  - @pnpm/workspace.workspace-manifest-reader@1100.2.1
