## 1100.0.51

### Patch Changes

- After a workspace script runs, pnpm links commands from scoped injected packages into the containing modules directory's `.bin`. Commands declared through `directories.bin` are also refreshed.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.44
  - @pnpm/bins.remover@1100.0.34
  - @pnpm/bins.resolver@1100.0.22
  - @pnpm/error@1100.2.3
  - @pnpm/fetching.directory-fetcher@1100.0.46
  - @pnpm/fs.graceful-fs@1100.2.7
  - @pnpm/fs.indexed-pkg-importer@1100.0.41
  - @pnpm/installing.modules-yaml@1101.0.11
  - @pnpm/pkg-manifest.reader@1100.0.26
  - @pnpm/types@1102.1.5
  - @pnpm/workspace.projects-reader@1101.1.9
