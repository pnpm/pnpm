---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed `pnpm update` removing transitive lockfile entries when `dedupePeerDependents` is disabled and the selected package is absent [pnpm/pnpm#12456](https://github.com/pnpm/pnpm/issues/12456).
