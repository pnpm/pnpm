---
"@pnpm/resolving.npm-resolver": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.package-requester": patch
"@pnpm/store.controller-types": patch
"pnpm": patch
---

Fix `minimumReleaseAge` / `resolutionMode: time-based` installs failing on lockfiles whose `time:` block is missing entries. The npm-resolver's peek-from-store fast path now surfaces `publishedAt` from the lockfile rather than discarding it, and falls through to a registry metadata fetch when the time-based cutoff can't be computed from the data on hand.
