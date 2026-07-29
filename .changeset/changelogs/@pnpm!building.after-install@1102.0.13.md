## 1102.0.13

### Patch Changes

- Installing a local `file:` directory dependency with the global virtual store enabled no longer fails with `TypeError: Cannot read properties of undefined (reading 'split')` [#13335](https://github.com/pnpm/pnpm/issues/13335).

  Local directory dependencies — `file:` directories and injected workspace packages — now get a global-virtual-store slot of their own per project. They used to share one slot across every project that depended on a directory of the same name, so a project could end up linked to another project's copy of the dependency.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.23
  - @pnpm/building.pkg-requires-build@1100.0.12
  - @pnpm/building.policy@1100.0.16
  - @pnpm/config.normalize-registries@1100.0.12
  - @pnpm/config.reader@1101.15.0
  - @pnpm/core-loggers@1100.3.0
  - @pnpm/deps.graph-hasher@1100.2.13
  - @pnpm/deps.path@1100.0.12
  - @pnpm/exec.lifecycle@1100.1.9
  - @pnpm/installing.context@1100.0.29
  - @pnpm/installing.modules-yaml@1100.0.13
  - @pnpm/lockfile.types@1100.0.17
  - @pnpm/lockfile.utils@1100.1.6
  - @pnpm/lockfile.walker@1100.0.17
  - @pnpm/pkg-manifest.reader@1100.0.13
  - @pnpm/store.cafs@1100.1.16
  - @pnpm/store.connection-manager@1100.3.13
  - @pnpm/store.controller-types@1100.1.11
  - @pnpm/types@1101.7.0
