## 1100.1.11

### Patch Changes

- `pnpm outdated` and `pnpm update` now apply `minimumReleaseAge` to GitHub Actions. `minimumReleaseAgeExclude` entries match action names such as `actions/checkout` [#13923](https://github.com/pnpm/pnpm/issues/13923).

- Updated dependencies:
  - @pnpm/config.version-policy@1100.2.4
  - @pnpm/error@1100.2.0
  - @pnpm/network.git-utils@1100.0.5
  - @pnpm/resolving.git-resolver@1100.1.23
  - @pnpm/types@1102.1.1
