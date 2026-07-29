## 1100.1.0

### Minor Changes

- Fixed an installed optional dependency being left without one of its own required dependencies. When a package reached through `optionalDependencies` is installable on the current system but one of its regular `dependencies` is not, a lockfile-based install skipped that dependency and installed the parent anyway, so importing the parent failed with `MODULE_NOT_FOUND`. The dependency is now installed, and an install-check warning reports the incompatibility. A dependency is still only skipped when every path to it is optional, or when the package that pulls it in was itself skipped [#13286](https://github.com/pnpm/pnpm/issues/13286).

### Patch Changes

- Installing a local `file:` directory dependency with the global virtual store enabled no longer fails with `TypeError: Cannot read properties of undefined (reading 'split')` [#13335](https://github.com/pnpm/pnpm/issues/13335).

  Local directory dependencies — `file:` directories and injected workspace packages — now get a global-virtual-store slot of their own per project. They used to share one slot across every project that depended on a directory of the same name, so a project could end up linked to another project's copy of the dependency.

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.1.0
  - @pnpm/core-loggers@1100.3.0
  - @pnpm/deps.graph-hasher@1100.2.13
  - @pnpm/deps.path@1100.0.12
  - @pnpm/fs.symlink-dependency@1100.0.15
  - @pnpm/hooks.types@1100.2.4
  - @pnpm/installing.modules-yaml@1100.0.13
  - @pnpm/lockfile.fs@1100.1.15
  - @pnpm/lockfile.utils@1100.1.6
  - @pnpm/patching.config@1100.0.13
  - @pnpm/store.controller-types@1100.1.11
  - @pnpm/types@1101.7.0
