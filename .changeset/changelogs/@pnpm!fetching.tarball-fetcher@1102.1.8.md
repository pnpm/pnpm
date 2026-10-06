## 1102.1.8

### Patch Changes

- Large package downloads and large files inside gzip and bzip2 package archives now use bounded memory during installation. Package manifests and archive metadata larger than 64 MiB are rejected.

- Updated dependencies:
  - @pnpm/error@1100.2.2
  - @pnpm/exec.prepare-package@1100.0.42
  - @pnpm/fs.graceful-fs@1100.2.5
  - @pnpm/fs.packlist@1100.0.8
  - @pnpm/network.fetch@1100.1.22
  - @pnpm/store.index@1101.0.1
