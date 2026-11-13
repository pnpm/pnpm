---
"@pnpm/resolving.npm-resolver": patch
"@pnpm/constants": patch
"@pnpm/cache.commands": patch
"pnpm": patch
"pacquet": patch
---

Fixed an offline install failing the `minimumReleaseAge` lockfile check with `ERR_PNPM_NO_OFFLINE_META` for packages that an online install had checked with the same cache directory. pnpm now saves the publish times it reads from the registry's attestation endpoint in the cache. The offline check also reads publish times from cached metadata that an earlier resolution stored [#18063](https://github.com/pnpm/pnpm/issues/18063).
