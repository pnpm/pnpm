## 1100.2.10

### Patch Changes

- `pnpm install --dev` and `pnpm fetch --dev` now install the optional dependencies of devDependencies, such as the platform binaries of Biome and oxlint. The project's own `optionalDependencies` are still skipped [#9678](https://github.com/pnpm/pnpm/issues/9678).

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.2.1
  - @pnpm/deps.path@1101.0.5
  - @pnpm/error@1100.2.1
  - @pnpm/lockfile.peer-edges@1100.0.1
  - @pnpm/lockfile.types@1100.1.4
  - @pnpm/lockfile.utils@1102.1.5
  - @pnpm/lockfile.walker@1100.0.26
