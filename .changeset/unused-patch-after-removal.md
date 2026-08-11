---
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

Dropping the last dependent of a patched package no longer leaves its `patchedDependencies` entry behind. Removing a dependency, widening `ignoredOptionalDependencies`, or adding a removal override could each prune the package while the lockfile kept the patch, where a full resolution reports `ERR_PNPM_UNUSED_PATCH` [#13827](https://github.com/pnpm/pnpm/issues/13827).
