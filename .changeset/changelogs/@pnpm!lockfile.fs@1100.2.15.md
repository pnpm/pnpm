## 1100.2.15

### Patch Changes

- `pnpm store prune` now keeps the packages that a project still uses when pnpm copied or cloned them into `node_modules`. These include patched packages, built packages, and every package installed with `packageImportMethod: copy`. Before, the prune removed them, and the next install downloaded them again [#7192](https://github.com/pnpm/pnpm/issues/7192).

- Updated dependencies:
  - @pnpm/deps.path@1101.0.8
  - @pnpm/lockfile.merger@1100.0.27
  - @pnpm/lockfile.types@1100.1.6
  - @pnpm/lockfile.utils@1102.1.8
  - @pnpm/patching.config@1100.1.11
  - @pnpm/types@1102.1.3
