## 1101.0.7

### Patch Changes

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/cli.utils@1101.0.30
  - @pnpm/config.reader@1102.3.1
  - @pnpm/deps.github-actions@1100.1.12
  - @pnpm/deps.inspection.list@1101.0.7
  - @pnpm/deps.inspection.outdated@1100.1.33
  - @pnpm/deps.inspection.peers-checker@1100.0.36
  - @pnpm/error@1100.2.1
  - @pnpm/global.commands@1102.0.4
  - @pnpm/global.packages@1101.1.5
  - @pnpm/installing.modules-yaml@1101.0.5
  - @pnpm/lockfile.fs@1100.2.10
  - @pnpm/network.auth-header@1101.1.15
  - @pnpm/network.fetch@1100.1.19
  - @pnpm/resolving.default-resolver@1101.0.7
  - @pnpm/resolving.npm-resolver@1104.2.2
  - @pnpm/resolving.registry.types@1100.2.2
  - @pnpm/store.path@1100.1.0
  - @pnpm/workspace.project-manifest-reader@1100.1.1
