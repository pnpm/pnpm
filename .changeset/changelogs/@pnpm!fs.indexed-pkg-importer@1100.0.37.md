## 1100.0.37

### Patch Changes

- `pnpm prune --prod` no longer fails with `ERR_PNPM_EXDEV` when `node_modules` was copied from an earlier Docker build stage [#17424](https://github.com/pnpm/pnpm/issues/17424).
