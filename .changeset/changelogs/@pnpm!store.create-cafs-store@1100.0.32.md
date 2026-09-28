## 1100.0.32

### Patch Changes

- `pnpm deploy` with a shared lockfile now copies workspace dependencies into the deploy directory, even when `packageImportMethod` is set to `hardlink`. Previously, their files were hard-linked to the workspace sources, so editing a source file also changed the deployed copy [pnpm/pnpm#12176](https://github.com/pnpm/pnpm/issues/12176).

- `pnpm install` no longer fails for an injected workspace dependency whose package publishes from a `publishConfig.directory` that its own `prepare` script builds. The injected copy now picks up that directory once `prepare` finishes building it. `pnpm install --frozen-lockfile` no longer reports the dependency as outdated while the directory has not been built yet. [pnpm/pnpm#7811](https://github.com/pnpm/pnpm/issues/7811)

- The side-effects cache now restores the symlinks that a build script creates inside a package. A warm install used to replace each of them with a copy of its target [#12859](https://github.com/pnpm/pnpm/issues/12859).

  After upgrading, every package with a build script is built once more.

- Updated dependencies:
  - @pnpm/building.pkg-requires-build@1100.0.19
  - @pnpm/fetching.fetcher-base@1100.2.12
  - @pnpm/fs.indexed-pkg-importer@1100.0.31
  - @pnpm/store.cafs@1100.3.5
  - @pnpm/store.controller-types@1101.3.2
