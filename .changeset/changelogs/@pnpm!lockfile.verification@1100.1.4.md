## 1100.1.4

### Patch Changes

- `pnpm install` now relinks workspace packages when `publishConfig.linkDirectory` changes. Frozen installs report an outdated lockfile until it is regenerated [pnpm/pnpm#14488](https://github.com/pnpm/pnpm/issues/14488).

- Updated dependencies:
  - @pnpm/installing.context@1101.0.3
  - @pnpm/lockfile.types@1100.1.1
  - @pnpm/lockfile.utils@1102.1.1
  - @pnpm/pkg-manifest.reader@1100.0.19
