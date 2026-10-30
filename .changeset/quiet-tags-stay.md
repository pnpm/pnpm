---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

`pnpm add` no longer rewrites a catalog entry that is set to a dist-tag, such as `beta` or `latest`. Before, the entry was replaced with the version the tag pointed to [pnpm/tasks#225](https://github.com/pnpm/tasks/issues/225).
