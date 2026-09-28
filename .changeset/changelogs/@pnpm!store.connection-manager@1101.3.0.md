## 1101.3.0

### Minor Changes

- pnpm now warns when it cannot hard link packages from an existing store in the pnpm home directory and uses a store on the project's filesystem instead. This can happen when the project is on another filesystem, such as a bind-mounted workspace in a container. The warning names both stores and suggests setting `storeDir` [#14505](https://github.com/pnpm/pnpm/issues/14505).

### Patch Changes

- `pnpm deploy` with a shared lockfile now copies workspace dependencies into the deploy directory, even when `packageImportMethod` is set to `hardlink`. Previously, their files were hard-linked to the workspace sources, so editing a source file also changed the deployed copy [pnpm/pnpm#12176](https://github.com/pnpm/pnpm/issues/12176).

- Updated dependencies:
  - @pnpm/config.reader@1102.3.1
  - @pnpm/installing.client@1100.3.12
  - @pnpm/resolving.resolver-base@1101.3.2
  - @pnpm/store.controller@1102.2.1
  - @pnpm/store.index@1100.3.3
  - @pnpm/store.path@1100.1.0
