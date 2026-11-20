---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

Fixed an offline install failing the `minimumReleaseAge` lockfile check with `ERR_PNPM_NO_OFFLINE_META` after an online install resolved the same packages with the same cache directory. The check now reads publish times from the metadata that the resolution cached [#18063](https://github.com/pnpm/pnpm/issues/18063).
