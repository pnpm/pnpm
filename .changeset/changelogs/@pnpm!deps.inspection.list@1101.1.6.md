## 1101.1.6

### Patch Changes

- `pnpm list --lockfile-only --find-by` now reads manifests from the configured store when `node_modules` is absent.

- `pnpm why --exclude-peers` now leaves out packages that are reached only as a resolved peer dependency. Previously the flag was accepted but ignored.

- Updated dependencies:
  - @pnpm/deps.inspection.tree-builder@1101.1.6
  - @pnpm/lockfile.fs@1100.2.20
  - @pnpm/pkg-manifest.reader@1100.0.26
  - @pnpm/types@1102.1.5
  - @pnpm/workspace.project-manifest-reader@1100.1.8
