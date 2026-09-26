## 1101.1.4

### Patch Changes

- `pnpm add -g`, `pnpm update -g`, and `pnpm remove -g` no longer fail with `ERR_PNPM_PACKAGE_MANIFEST_IO_ERROR` when another global package's link into the store dangles, for example after the store was pruned. The pnpm install script failed the same way on such a machine.

- Updated dependencies:
  - @pnpm/bins.resolver@1100.0.17
  - @pnpm/crypto.hash@1100.0.6
  - @pnpm/pkg-manifest.reader@1100.0.20
  - @pnpm/types@1102.1.1
