## 1100.1.5

### Patch Changes

- `pnpm pkg get` and `pnpm pkg set` now accept hyphens inside a dot-notation property path, so `pnpm pkg get dependencies.some-package-name` reads the key instead of failing with `ERR_PNPM_UNEXPECTED_TOKEN_IN_PROPERTY_PATH`. The bracketed and quoted forms already worked and are unchanged.

- Updated dependencies:
  - @pnpm/error@1100.1.3
