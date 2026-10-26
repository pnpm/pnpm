## 1102.2.7

### Patch Changes

- `pnpm store prune` now keeps the packages that a project still uses when pnpm copied or cloned them into `node_modules`. These include patched packages, built packages, and every package installed with `packageImportMethod: copy`. Before, the prune removed them, and the next install downloaded them again [#7192](https://github.com/pnpm/pnpm/issues/7192).

- `pnpm store prune` now keeps the packages of a project that was installed with a different symlinked spelling of the store path. Previously, prune dropped such a project from its list and removed the packages only that project used from the global virtual store.

- Updated dependencies:
  - @pnpm/config.reader@1102.3.7
  - @pnpm/fetching.fetcher-base@1100.2.14
  - @pnpm/hooks.types@1101.0.7
  - @pnpm/installing.modules-yaml@1101.0.8
  - @pnpm/installing.package-requester@1102.2.7
  - @pnpm/lockfile.fs@1100.2.15
  - @pnpm/lockfile.types@1100.1.6
  - @pnpm/lockfile.utils@1102.1.8
  - @pnpm/resolving.resolver-base@1101.3.4
  - @pnpm/store.cafs@1100.3.9
  - @pnpm/store.controller-types@1101.3.5
  - @pnpm/store.create-cafs-store@1100.0.37
  - @pnpm/types@1102.1.3
  - @pnpm/workspace.project-manifest-reader@1100.1.5
  - @pnpm/workspace.workspace-manifest-reader@1100.3.1
