## 1100.0.36

### Patch Changes

- `pnpm deploy` with a shared lockfile now copies workspace dependencies into the deploy directory, even when `packageImportMethod` is set to `hardlink`. Previously, their files were hard-linked to the workspace sources, so editing a source file also changed the deployed copy [pnpm/pnpm#12176](https://github.com/pnpm/pnpm/issues/12176).

- `pnpm install` no longer fails for an injected workspace dependency whose package publishes from a `publishConfig.directory` that its own `prepare` script builds. The injected copy now picks up that directory once `prepare` finishes building it. `pnpm install --frozen-lockfile` no longer reports the dependency as outdated while the directory has not been built yet. [pnpm/pnpm#7811](https://github.com/pnpm/pnpm/issues/7811)

- `pnpm install` now works in StackBlitz WebContainers on projects without a lockfile. It used to fail there with `ENOENT ... pnpm-lock.yaml`, because pnpm did not recognize the errors that WebContainers return from asynchronous file system calls [#15649](https://github.com/pnpm/pnpm/issues/15649).

- Updated dependencies:
  - @pnpm/building.pkg-requires-build@1100.0.19
  - @pnpm/error@1100.2.1
  - @pnpm/fetching.fetcher-base@1100.2.12
  - @pnpm/fs.packlist@1100.0.6
  - @pnpm/resolving.resolver-base@1101.3.2
  - @pnpm/store.cafs-types@1100.1.1
  - @pnpm/workspace.project-manifest-reader@1100.1.1
