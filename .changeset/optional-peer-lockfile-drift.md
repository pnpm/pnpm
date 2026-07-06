---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm install` no longer re-propagates an optional transitive peer dependency onto unrelated packages when regenerating the lockfile after a manifest edit. The locked-peer-context reuse pass now skips an optional peer whose provider is not visible in the resolving package's scope, matching a fresh resolution and `pnpm dedupe`. This fixes lockfile drift where an unrelated `pnpm install` would add peer suffixes that `pnpm dedupe` would immediately remove.
