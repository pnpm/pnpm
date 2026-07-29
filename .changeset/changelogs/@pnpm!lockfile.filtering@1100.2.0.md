## 1100.2.0

### Minor Changes

- Fixed an installed optional dependency being left without one of its own required dependencies. When a package reached through `optionalDependencies` is installable on the current system but one of its regular `dependencies` is not, a lockfile-based install skipped that dependency and installed the parent anyway, so importing the parent failed with `MODULE_NOT_FOUND`. The dependency is now installed, and an install-check warning reports the incompatibility. A dependency is still only skipped when every path to it is optional, or when the package that pulls it in was itself skipped [#13286](https://github.com/pnpm/pnpm/issues/13286).

### Patch Changes

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.1.0
  - @pnpm/deps.path@1100.0.12
  - @pnpm/lockfile.types@1100.0.17
  - @pnpm/lockfile.utils@1100.1.6
  - @pnpm/lockfile.walker@1100.0.17
  - @pnpm/types@1101.7.0
