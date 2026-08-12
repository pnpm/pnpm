---
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

Changing a parent-scoped `pnpm.overrides` entry (`"parent>child": "2.0.0"`) now updates the lockfile in place instead of re-resolving the whole dependency graph. Only the named parent's dependency moves; every other package keeps the version it had [#13795](https://github.com/pnpm/pnpm/issues/13795).
