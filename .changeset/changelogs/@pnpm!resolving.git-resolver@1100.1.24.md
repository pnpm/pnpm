## 1100.1.24

### Patch Changes

- When installing a git dependency over SSH fails with `Permission denied (publickey)`, pnpm suggests checking the loaded keys with `ssh-add -l`.

  Resolving an SSH URL that refuses the key also shows a local HTTPS rewrite that leaves the recorded URL alone [pnpm/pnpm#13743](https://github.com/pnpm/pnpm/issues/13743).

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/error@1100.2.1
  - @pnpm/network.fetch@1100.1.19
  - @pnpm/network.git-utils@1100.0.6
  - @pnpm/resolving.resolver-base@1101.3.2
