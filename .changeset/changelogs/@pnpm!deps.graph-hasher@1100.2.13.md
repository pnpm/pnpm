## 1100.2.13

### Patch Changes

- Installing a local `file:` directory dependency with the global virtual store enabled no longer fails with `TypeError: Cannot read properties of undefined (reading 'split')` [#13335](https://github.com/pnpm/pnpm/issues/13335).

  Local directory dependencies — `file:` directories and injected workspace packages — now get a global-virtual-store slot of their own per project. They used to share one slot across every project that depended on a directory of the same name, so a project could end up linked to another project's copy of the dependency.

- Updated dependencies:
  - @pnpm/deps.path@1100.0.12
  - @pnpm/engine.runtime.system-version@1100.0.7
  - @pnpm/lockfile.types@1100.0.17
  - @pnpm/lockfile.utils@1100.1.6
  - @pnpm/resolving.resolver-base@1100.5.5
  - @pnpm/types@1101.7.0
