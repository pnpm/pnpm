---
"@pnpm/lockfile.pruner": patch
"pnpm": patch
---

Fixed `pruneLockfile()` and `pruneSharedLockfile()` in `@pnpm/lockfile.pruner` throwing `TypeError: reference.startsWith is not a function` when an importer's dependency entries used the inline `{ specifier, version }` shape from lockfile v9 [#10126](https://github.com/pnpm/pnpm/issues/10126).
