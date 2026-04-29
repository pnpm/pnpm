# @pnpm/testing.command-defaults

## 1100.0.1

### Patch Changes

- 9e0833c: Added a new setting `minimumReleaseAgeIgnoreMissingTime`, which is `true` by default. When enabled, pnpm skips the `minimumReleaseAge` maturity check if the registry metadata does not include the `time` field. Set to `false` to fail resolution instead.
