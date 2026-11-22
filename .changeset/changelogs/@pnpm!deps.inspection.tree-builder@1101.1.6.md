## 1101.1.6

### Patch Changes

- `pnpm list --lockfile-only --find-by` now reads manifests from the configured store when `node_modules` is absent.

- `pnpm list --json` now reports unsaved dependencies from each workspace project's own modules directory [#18248](https://github.com/pnpm/pnpm/issues/18248).

- `pnpm why --exclude-peers` now leaves out packages that are reached only as a resolved peer dependency. Previously the flag was accepted but ignored.

- Updated dependencies:
  - @pnpm/config.normalize-registries@1101.0.7
  - @pnpm/deps.path@1101.0.11
  - @pnpm/fs.read-modules-dir@1100.0.5
  - @pnpm/installing.modules-yaml@1101.0.11
  - @pnpm/lockfile.detect-dep-types@1100.0.32
  - @pnpm/lockfile.fs@1100.2.20
  - @pnpm/lockfile.peer-edges@1100.0.7
  - @pnpm/lockfile.utils@1102.1.13
  - @pnpm/pkg-manifest.reader@1100.0.26
  - @pnpm/store.cafs@1100.3.12
  - @pnpm/store.index@1101.0.5
  - @pnpm/types@1102.1.5
  - @pnpm/workspace.state@1100.0.57
