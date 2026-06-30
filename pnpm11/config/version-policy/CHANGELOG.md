# @pnpm/config.version-policy

## 1100.1.6

### Patch Changes

- 25a829e: `pnpm audit --fix` now writes a single combined `minimumReleaseAgeExclude` entry per package (e.g. `axios@0.18.1 || 0.21.1`) instead of one entry per version, matching the format documented for the setting. Existing per-version entries in `pnpm-workspace.yaml` are merged into the combined form rather than left as duplicates. Installs that auto-collect immature versions into `minimumReleaseAgeExclude` now report the same combined entries, so the "Added N entries" message matches what is written to the manifest [#12534](https://github.com/pnpm/pnpm/issues/12534).
- fbdc0eb: Fixed `minimumReleaseAgeExclude` and `trustPolicyExclude` so multiple exact-version entries for the same package behave the same as a single `||` disjunction entry. Previously only the first matching rule's versions were honored, so a config like `[form-data@4.0.6, form-data@2.5.6]` could still flag `form-data@2.5.6` as violating `minimumReleaseAge`, while `[form-data@4.0.6 || 2.5.6]` worked as expected [#12463](https://github.com/pnpm/pnpm/issues/12463).
- Updated dependencies [852d537]
  - @pnpm/error@1100.0.1

## 1100.1.5

### Patch Changes

- a31faa7: Updated dependency ranges. Notably:

  - `@pnpm/logger` peer dependency range moved to `^1100.0.0`.
  - `msgpackr` 1.11.8 → 2.0.4 (store index files remain byte-compatible in both directions).
  - `open` ^7.4.2 → ^11.0.0, `memoize` ^10 → ^11, `cli-truncate` ^5 → ^6, `pidtree` ^0.6 → ^1.
  - `@yarnpkg/core` 4.5.0 → 4.8.0, `@rushstack/worker-pool` 0.7.7 → 0.7.18, `@cyclonedx/cyclonedx-library` 10.0.0 → 10.1.0, `@pnpm/config.nerf-dart` ^1 → ^2, `@pnpm/log.group` 3.0.2 → 4.0.1, `@pnpm/util.lex-comparator` ^3 → ^4.

- Updated dependencies [681b593]
  - @pnpm/types@1101.3.2

## 1100.1.4

### Patch Changes

- Updated dependencies [bf1b731]
  - @pnpm/types@1101.3.1

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
