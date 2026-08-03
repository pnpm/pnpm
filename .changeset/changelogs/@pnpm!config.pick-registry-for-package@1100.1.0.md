## 1100.1.0

### Minor Changes

- Fixed the order in which pnpm matches a lockfile's recorded tarball URL against known registry URLs. Two registry URLs of equal length were previously ordered arbitrarily, so which one a tarball URL matched could differ between runs.

### Patch Changes

- Updated dependencies:
  - @pnpm/types@1101.9.0
