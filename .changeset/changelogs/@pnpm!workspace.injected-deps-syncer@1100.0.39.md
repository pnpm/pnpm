## 1100.0.39

### Patch Changes

- `syncInjectedDepsAfterScripts` now copies files into injected dependencies when `node_modules` is on another filesystem than the package sources. The sync previously failed with a cross-device link error and made the script run exit with an error [pnpm/pnpm#14703](https://github.com/pnpm/pnpm/issues/14703).

- Fixed injected workspace dependency synchronization failing with `EPERM` on Windows when removing nested directories.

- Updated dependencies:
  - @pnpm/bins.linker@1100.0.33
  - @pnpm/bins.remover@1100.0.25
  - @pnpm/bins.resolver@1100.0.17
  - @pnpm/error@1100.2.0
  - @pnpm/fetching.directory-fetcher@1100.0.35
  - @pnpm/fs.graceful-fs@1100.2.3
  - @pnpm/installing.modules-yaml@1101.0.4
  - @pnpm/pkg-manifest.reader@1100.0.20
  - @pnpm/types@1102.1.1
  - @pnpm/workspace.projects-reader@1101.1.0
