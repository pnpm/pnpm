## 1100.0.26

### Patch Changes

- `pnpm install --dev` and `pnpm fetch --dev` now install the optional dependencies of devDependencies, such as the platform binaries of Biome and oxlint. The project's own `optionalDependencies` are still skipped [#9678](https://github.com/pnpm/pnpm/issues/9678).

- Updated dependencies:
  - @pnpm/deps.path@1101.0.5
  - @pnpm/lockfile.peer-edges@1100.0.1
  - @pnpm/lockfile.types@1100.1.4
