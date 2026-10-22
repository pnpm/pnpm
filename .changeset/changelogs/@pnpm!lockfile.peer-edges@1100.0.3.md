## 1100.0.3

### Patch Changes

- `pnpm install --prod` no longer drops a dependency that a package declares both as an optional peer and as its own dependency, when the package resolved it as its own dependency and a dev dependency provides the same package under another alias.

- Updated dependencies:
  - @pnpm/deps.path@1101.0.7
  - @pnpm/lockfile.types@1100.1.5
  - @pnpm/types@1102.1.2
