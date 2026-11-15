## 1100.1.42

### Patch Changes

- Version-selector `overrides` entries such as `"ms@^2.0.0": "2.1.1"` now apply to dependencies declared with the `catalog:` protocol when the catalog entry's range matches the selector [#17922](https://github.com/pnpm/pnpm/issues/17922).

- Updated dependencies:
  - @pnpm/deps.path@1101.0.9
  - @pnpm/hooks.read-package-hook@1100.3.12
  - @pnpm/installing.client@1100.3.21
  - @pnpm/lockfile.fs@1100.2.18
  - @pnpm/lockfile.utils@1102.1.11
  - @pnpm/pkg-manifest.utils@1100.4.12
  - @pnpm/resolving.npm-resolver@1104.2.10
