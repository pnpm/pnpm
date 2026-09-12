---
"pacquet": patch
---

`pnpm install` no longer fails with `Operation not permitted` when the filesystem refuses a hardlink or a reflink. Under `packageImportMethod: auto` and `clone-or-copy`, pnpm copies the file. EdenFS checkouts, which have no hardlinks, and rootless containers, which refuse the clone syscall, both hit this. An explicit `packageImportMethod: hardlink` or `clone` still reports the error [#14722](https://github.com/pnpm/pnpm/issues/14722).
