## 1100.3.12

### Patch Changes

- Version-selector `overrides` entries such as `"ms@^2.0.0": "2.1.1"` now apply to dependencies declared with the `catalog:` protocol when the catalog entry's range matches the selector [#17922](https://github.com/pnpm/pnpm/issues/17922).

- Updated dependencies:
  - @pnpm/resolving.local-resolver@1101.2.9
