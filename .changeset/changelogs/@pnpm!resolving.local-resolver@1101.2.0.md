## 1101.2.0

### Minor Changes

- `catalogMode` and `--save-catalog` no longer move a local path, tarball, or `workspace:<path>` specifier into a catalog. Such a specifier is resolved against the project that declares it, so one catalog entry cannot mean the same directory for every project that references it [#14437](https://github.com/pnpm/pnpm/issues/14437).

### Patch Changes

- Updated dependencies:
  - @pnpm/error@1100.1.4
  - @pnpm/workspace.project-manifest-reader@1100.0.27
