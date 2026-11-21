---
"@pnpm/napi": minor
---

The `getDependents` options accept `excludePeerDependencies`, which leaves out the packages reached only as a resolved peer dependency, as `pnpm why --exclude-peers` does.
