---
"@pnpm/resolving.local-resolver": minor
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

`catalogMode` and `--save-catalog` no longer move a local path or tarball specifier into a catalog. A catalog entry is shared by every project that references it, so it cannot hold a path that resolves against the project declaring it — cataloging one wrote an entry the next install rejected with `ERR_PNPM_CATALOG_ENTRY_INVALID_SPEC` [#14437](https://github.com/pnpm/pnpm/issues/14437).
