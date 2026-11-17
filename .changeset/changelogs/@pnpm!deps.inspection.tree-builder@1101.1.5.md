## 1101.1.5

### Patch Changes

- `pnpm why` no longer merges distinct local `file:` directory dependencies that share the same package name across projects with dedicated lockfiles (`sharedWorkspaceLockfile: false`).

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.6
  - @pnpm/deps.path@1101.0.10
  - @pnpm/installing.modules-yaml@1101.0.10
  - @pnpm/lockfile.detect-dep-types@1100.0.31
  - @pnpm/lockfile.fs@1100.2.19
  - @pnpm/lockfile.peer-edges@1100.0.6
  - @pnpm/lockfile.utils@1102.1.12
  - @pnpm/pkg-manifest.reader@1100.0.25
  - @pnpm/store.cafs@1100.3.11
  - @pnpm/types@1102.1.4
