## 1100.3.7

### Patch Changes

- Large package downloads and large files inside gzip and bzip2 package archives now use bounded memory during installation. Package manifests and archive metadata larger than 64 MiB are rejected.

- Prevent bundled dependencies from traversing escaping directory symlinks or packaging files outside the package root [pnpm/tasks#83](https://github.com/pnpm/tasks/issues/83) [pnpm/tasks#93](https://github.com/pnpm/tasks/issues/93).

- Validate PAX header record lengths and use 64-bit modulo arithmetic for tar block padding [pnpm/tasks#78](https://github.com/pnpm/tasks/issues/78) [pnpm/tasks#79](https://github.com/pnpm/tasks/issues/79).

- Updated dependencies:
  - @pnpm/error@1100.2.2
  - @pnpm/fs.graceful-fs@1100.2.5
  - @pnpm/store.file-mode@1100.0.1
