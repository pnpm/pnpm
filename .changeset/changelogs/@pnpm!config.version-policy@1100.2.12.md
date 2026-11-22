## 1100.2.12

### Patch Changes

- Added the `--minimum-release-age` and `--minimum-release-age-exclude` options to `pnpm install`, `pnpm add`, `pnpm update`, and `pnpm dlx`. They set `minimumReleaseAge` and `minimumReleaseAgeExclude` for a single run without editing a config file [#11224](https://github.com/pnpm/pnpm/issues/11224).

- `minimumReleaseAge` must now be a finite, non-negative number of minutes. Other values fail with `ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE`. Before, such a value turned off the release age check or crashed the install.

- Updated dependencies:
  - @pnpm/error@1100.2.3
  - @pnpm/types@1102.1.5
