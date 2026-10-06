## 1100.4.8

### Patch Changes

- Large package downloads and large files inside gzip and bzip2 package archives now use bounded memory during installation. Package manifests and archive metadata larger than 64 MiB are rejected.

- Updated dependencies:
  - @pnpm/building.pkg-requires-build@1100.0.20
  - @pnpm/crypto.integrity@1100.0.9
  - @pnpm/error@1100.2.2
  - @pnpm/fs.graceful-fs@1100.2.5
  - @pnpm/fs.hard-link-dir@1100.0.9
  - @pnpm/fs.symlink-dependency@1100.0.24
  - @pnpm/store.cafs@1100.3.7
  - @pnpm/store.create-cafs-store@1100.0.34
  - @pnpm/store.index@1101.0.1
