---
"@pnpm/lockfile.settings-checker": patch
"pnpm": patch
---

Checking whether `ignoredOptionalDependencies` is up to date no longer reorders the configured patterns. The check sorted them in place, which could move an `!` exclusion ahead of the pattern it excludes from and flip which optional dependencies were ignored.
