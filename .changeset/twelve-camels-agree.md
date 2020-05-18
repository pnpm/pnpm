---
"@pnpm/resolve-dependencies": patch
"supi": patch
---

The lockfile should be recreated correctly when an up-to-date `node_modules` is present.
The recreated lockfile should contain all the skipped optional dependencies.
