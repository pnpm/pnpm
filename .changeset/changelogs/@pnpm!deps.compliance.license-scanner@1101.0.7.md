## 1101.0.7

### Patch Changes

- `pnpm licenses list --json` now includes every installed copy of a package in its `paths` array. Multiple copies of the same version previously contributed only one path. This includes hoisted copies and isolated installations with different peer dependencies.

- `pnpm licenses list --json` now reports existing package paths when the isolated linker uses a custom `modulesDir`. The reported paths previously used the custom directory name inside virtual-store slots.

- `pnpm licenses list` now reports the actual on-disk package locations when using `nodeLinker: hoisted` or `shamefully-hoist: true` [pnpm/pnpm#8589](https://github.com/pnpm/pnpm/issues/8589).

- Updated dependencies:
  - @pnpm/config.package-is-installable@1100.2.1
  - @pnpm/deps.path@1101.0.5
  - @pnpm/error@1100.2.1
  - @pnpm/installing.modules-yaml@1101.0.5
  - @pnpm/lockfile.detect-dep-types@1100.0.26
  - @pnpm/lockfile.fs@1100.2.10
  - @pnpm/lockfile.types@1100.1.4
  - @pnpm/lockfile.utils@1102.1.5
  - @pnpm/lockfile.walker@1100.0.26
  - @pnpm/pkg-manifest.reader@1100.0.21
  - @pnpm/store.index@1100.3.3
  - @pnpm/store.pkg-finder@1100.0.36
