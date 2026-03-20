---
"@pnpm/building.during-install": patch
"pnpm": patch
---

Fixed `strictDepBuilds` and `allowBuilds` checks being bypassed when a package's build side-effects are cached in the store. Previously, packages with cached builds were skipped entirely during the `allowBuild` check, so they never appeared in `ignoredBuilds` and `strictDepBuilds` would not fail for them.
