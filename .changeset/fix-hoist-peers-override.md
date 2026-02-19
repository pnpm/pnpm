---
"@pnpm/resolve-dependencies": patch
---

Fix auto-installed peer dependencies ignoring overrides when a stale version exists in the lockfile. Previously, `hoistPeers` used `semver.maxSatisfying(versions, '*')` which picked the highest preferred version regardless of the peer dep range. Now it first tries `semver.maxSatisfying(versions, range)` to respect the actual range (which may have been narrowed by an override).
