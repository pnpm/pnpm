## 1100.0.33

### Patch Changes

- `pnpm install` now applies changes to or removal of a global `readPackage` hook when an existing lockfile is present [pnpm/pnpm#15136](https://github.com/pnpm/pnpm/issues/15136).

- The project's `.pnpmfile.mjs` or `.pnpmfile.cjs` now runs after the pnpmfiles of config dependency plugins [#9891](https://github.com/pnpm/pnpm/issues/9891).

- A `readPackage` hook that sets a dependency range to a value other than a string, such as `undefined`, now fails the install with an error that names the dependency, the package, and the pnpmfile. Delete the property to remove a dependency [#5517](https://github.com/pnpm/pnpm/issues/5517).

- Updated dependencies:
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/crypto.hash@1100.0.6
  - @pnpm/error@1100.2.0
  - @pnpm/hooks.types@1101.0.4
  - @pnpm/lockfile.types@1100.1.3
  - @pnpm/store.controller-types@1101.3.1
  - @pnpm/types@1102.1.1
