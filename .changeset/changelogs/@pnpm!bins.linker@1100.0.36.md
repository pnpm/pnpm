## 1100.0.36

### Patch Changes

- `pnpm rebuild` and `pnpm approve-builds` refresh command launchers when a build changes a command's interpreter or replaces it with a native executable.

  Dependent packages' build scripts use the refreshed launchers.

- Updated dependencies:
  - @pnpm/pkg-manifest.utils@1100.4.8
  - @pnpm/workspace.project-manifest-reader@1100.1.2
