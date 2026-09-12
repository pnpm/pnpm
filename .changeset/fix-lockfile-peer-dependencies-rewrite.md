---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

A `readPackage` hook that edits its argument in place no longer changes what a later install in the same command resolves. A `deprecated` notice read from the lockfile no longer carries over to another install either [#13988](https://github.com/pnpm/pnpm/issues/13988).
