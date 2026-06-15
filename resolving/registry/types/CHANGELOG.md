# @pnpm/registry.types

## 1100.1.3

### Patch Changes

- Updated dependencies [681b593]
  - @pnpm/types@1101.3.2

## 1100.1.2

### Patch Changes

- Updated dependencies [bf1b731]
  - @pnpm/types@1101.3.1

## 1100.1.1

### Patch Changes

- Updated dependencies [a017bf3]
  - @pnpm/types@1101.3.0

## 1100.1.0

### Minor Changes

- 1e9ab29: Staged publishes are now recognized in the trust scale. When a package version's registry metadata carries an `approver` field, it is treated as the strongest trust evidence (ranked above trusted publishers and provenance attestations), since staged publishes require 2FA publish approvals. This prevents false-positive trust downgrade errors when moving from a staged publish to a lower trust level [#11887](https://github.com/pnpm/pnpm/issues/11887).

## 1100.0.5

### Patch Changes

- Updated dependencies [35d2355]
  - @pnpm/types@1101.2.0

## 1100.0.4

### Patch Changes

- Updated dependencies [64afc92]
  - @pnpm/types@1101.1.1

## 1100.0.3

### Patch Changes

- Updated dependencies [b61e268]
  - @pnpm/types@1101.1.0

## 1100.0.2

### Patch Changes

- 184ce26: Fix the package name in README.md.

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0

## 1000.1.0

### Minor Changes

- 10bc391: Added a new setting: `trustPolicy`.

### Patch Changes

- d3d6938: Added native `pnpm view` command with `info`, `show`, and `v` aliases for viewing package information from the registry. Supports version ranges, dist-tags, aliases, field selection, and JSON output.
- Updated dependencies [76718b3]
- Updated dependencies [a8f016c]
- Updated dependencies [cc1b8e3]
- Updated dependencies [491a84f]
- Updated dependencies [7d2fd48]
- Updated dependencies [efb48dc]
- Updated dependencies [cb367b9]
- Updated dependencies [7b1c189]
- Updated dependencies [8ffb1a7]
- Updated dependencies [05fb1ae]
- Updated dependencies [71de2b3]
- Updated dependencies [10bc391]
- Updated dependencies [2df8b71]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
  - @pnpm/types@1001.0.0

## 1000.0.1

### Patch Changes

- Updated dependencies [7c1382f]
- Updated dependencies [dee39ec]
  - @pnpm/types@1000.9.0

## 1000.0.0

### Major Changes

- 4a2d871: Initial release.
