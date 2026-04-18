---
"@pnpm/config.reader": minor
"@pnpm/resolving.npm-resolver": minor
"@pnpm/store.connection-manager": patch
"@pnpm/deps.inspection.outdated": patch
"@pnpm/exec.commands": patch
"@pnpm/testing.command-defaults": patch
"pnpm": minor
---

Added a new setting `minimumReleaseAgeIgnoreMissingTime`, which is `true` by default. When enabled, pnpm skips the `minimumReleaseAge` maturity check if the registry metadata does not include the `time` field. Set to `false` to fail resolution instead.
