## 1100.4.6

### Patch Changes

- `pnpm add` now saves the requested exact version when adding a dependency, even when the manifest already contains a version range [#6040](https://github.com/pnpm/pnpm/issues/6040).

- `pnpm update` now keeps a version range whose shape has no save prefix, such as `<= 3.0.0` or `>=1.0.0 <2.0.0`, when the updated version still satisfies it. Before, `<= 3.0.0` became `^3.0.0` [#6714](https://github.com/pnpm/pnpm/issues/6714).

- Added a `--peer` flag to `pnpm update` to update ranges in `peerDependencies` [#8081](https://github.com/pnpm/pnpm/issues/8081).

- Updated dependencies:
  - @pnpm/core-loggers@1101.0.1
  - @pnpm/error@1100.2.0
  - @pnpm/types@1102.1.1
