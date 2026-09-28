## 3.1.0

### Minor Changes

- Installing through a `pnpr` server now links a workspace project at the directory its `publishConfig.directory` names, instead of linking the project root. An install that resolves through a server which does not forward the setting fails with `ERR_PNPM_PNPR_PUBLISH_DIRECTORY_MISMATCH` instead of writing a lockfile that points at the wrong directory, and the server rejects a `publishConfig.directory` that points outside its project [#14460](https://github.com/pnpm/pnpm/issues/14460).

### Patch Changes

- Updated dependencies:
  - @pnpm/deps.graph-hasher@1100.3.5
  - @pnpm/error@1100.2.1
  - @pnpm/lockfile.fs@1100.2.10
  - @pnpm/lockfile.types@1100.1.4
  - @pnpm/network.auth-header@1101.1.15
  - @pnpm/store.cafs@1100.3.5
  - @pnpm/store.controller-types@1101.3.2
