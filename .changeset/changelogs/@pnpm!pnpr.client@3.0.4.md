## 3.0.4

### Patch Changes

- Installing through a pnpr server now installs a project's peer dependencies when `autoInstallPeers` is enabled. A project that declared only peer dependencies failed with `ERR_PNPM_OUTDATED_LOCKFILE` or skipped its peers [#14833](https://github.com/pnpm/pnpm/issues/14833).

- Updated dependencies:
  - @pnpm/deps.graph-hasher@1100.3.4
  - @pnpm/error@1100.2.0
  - @pnpm/lockfile.fs@1100.2.9
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/network.auth-header@1101.1.14
  - @pnpm/store.controller-types@1101.3.1
  - @pnpm/types@1102.1.1
