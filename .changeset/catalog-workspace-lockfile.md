---
"pnpm": patch
"@pnpm/installing.deps-resolver": patch
"pacquet": patch
---

Dependencies declared as `workspace:` entries in catalogs now remain in the lockfile when `excludeLinksFromLockfile` is enabled. Frozen installs can restore these links after `node_modules` is removed [#260](https://github.com/pnpm/tasks/issues/260).
