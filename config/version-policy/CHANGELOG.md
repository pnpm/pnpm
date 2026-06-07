# @pnpm/config.version-policy

## 1100.1.3

### Patch Changes

- Updated dependencies [a017bf3]
  - @pnpm/types@1101.3.0

## 1100.1.2

### Patch Changes

- Updated dependencies [35d2355]
  - @pnpm/types@1101.2.0

## 1100.1.1

### Patch Changes

- Updated dependencies [64afc92]
  - @pnpm/types@1101.1.1

## 1100.1.0

### Minor Changes

- b6e2c8c: Make `pnpm self-update` respect `minimumReleaseAge` (and `minimumReleaseAgeExclude`) when resolving which pnpm version to install.

  When the `latest` dist-tag points to a version newer than the configured age threshold, `self-update` now selects the newest mature version instead unless excluded by `minimumReleaseAgeExclude`.

  Also makes `dlx` and `outdated` surface invalid `minimumReleaseAgeExclude` patterns under the same `ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE` error code already used by `install`, instead of leaking the internal `ERR_PNPM_INVALID_VERSION_UNION` / `ERR_PNPM_NAME_PATTERN_IN_VERSION_UNION` codes.

## 1100.0.3

### Patch Changes

- Updated dependencies [b61e268]
  - @pnpm/types@1101.1.0

## 1100.0.2

### Patch Changes

- Updated dependencies [184ce26]
  - @pnpm/config.matcher@1100.0.1

## 1100.0.1

### Patch Changes

- Updated dependencies [ff28085]
  - @pnpm/types@1101.0.0

## 1000.0.1

### Patch Changes

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
- Updated dependencies [831f574]
- Updated dependencies [2df8b71]
- Updated dependencies [15549a9]
- Updated dependencies [cc7c0d2]
- Updated dependencies [efb48dc]
  - @pnpm/types@1001.0.0
  - @pnpm/config.matcher@1001.0.0
  - @pnpm/error@1001.0.0

## 1000.0.0

### Major Changes

- dee39ec: Initial release.

### Patch Changes

- Updated dependencies [7c1382f]
- Updated dependencies [dee39ec]
- Updated dependencies [7c1382f]
  - @pnpm/types@1000.9.0
  - @pnpm/matcher@1000.1.0
