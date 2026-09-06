## 1100.3.0

### Minor Changes

- Added `pnpm change check`. It validates the committed package versions against the `versioning.epics` bands and the `versioning.fixed` groups in `pnpm-workspace.yaml` and lists every violation. It is meant to run in CI, because `pnpm version -r` only checks the packages it releases.

### Patch Changes

- Updated dependencies:
  - @pnpm/error@1100.1.4
  - @pnpm/workspace.project-manifest-reader@1100.0.27
