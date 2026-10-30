---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm add` no longer rewrites a catalog entry set to the dist-tag that the dependency resolves through. For example, an entry `beta` stays `beta` after `pnpm add foo@beta` or `pnpm add foo`. Before, the entry was replaced with the version the tag pointed to [pnpm/tasks#225](https://github.com/pnpm/tasks/issues/225).
