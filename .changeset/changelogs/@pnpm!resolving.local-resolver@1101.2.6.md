## 1101.2.6

### Patch Changes

- `pnpm install` now fails with `ERR_PNPM_UNSUPPORTED_PROTOCOL` when a dependency uses a specifier with a protocol pnpm does not support, such as Yarn's `patch:`. pnpm linked such a dependency to a directory that does not exist [#16590](https://github.com/pnpm/pnpm/issues/16590).

- Updated dependencies:
  - @pnpm/crypto.hash@1100.0.8
  - @pnpm/deps.path@1101.0.6
  - @pnpm/error@1100.2.2
  - @pnpm/workspace.project-manifest-reader@1100.1.3
