## 1100.0.44

### Patch Changes

- After a workspace script runs, pnpm links commands from scoped injected packages into the containing modules directory's `.bin`. Commands declared through `directories.bin` are also refreshed.

- Updated dependencies:
  - @pnpm/bins.cmd-shim@1100.0.8
  - @pnpm/bins.resolver@1100.0.22
  - @pnpm/error@1100.2.3
  - @pnpm/fs.read-modules-dir@1100.0.5
  - @pnpm/pkg-manifest.reader@1100.0.26
  - @pnpm/pkg-manifest.utils@1100.4.14
  - @pnpm/types@1102.1.5
  - @pnpm/workspace.project-manifest-reader@1100.1.8
