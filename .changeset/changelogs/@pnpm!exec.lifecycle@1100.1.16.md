## 1100.1.16

### Patch Changes

- Fixed argument forwarding on Windows when `shellEmulator` is enabled. Paths ending in a backslash, line breaks, and literal shell expressions are preserved [pnpm/pnpm#14548](https://github.com/pnpm/pnpm/issues/14548).

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.30
  - @pnpm/error@1100.1.4
  - @pnpm/fetching.directory-fetcher@1100.0.32
  - @pnpm/pkg-manifest.reader@1100.0.19
  - @pnpm/workspace.task-scheduler@1100.0.1
