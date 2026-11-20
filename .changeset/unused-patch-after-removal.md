---
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

An install that drops the last dependent of a patched package no longer updates the lockfile in place and succeeds silently. Removing a dependency, widening `ignoredOptionalDependencies`, or adding a removal override could each prune the package while the patch stayed configured; such an install now falls back to a full resolution, which reports the unused patch with `ERR_PNPM_UNUSED_PATCH`. Under `allowUnusedPatches`, where the lockfile update is kept, the same install now warns that the patch went unused instead of saying nothing [#13827](https://github.com/pnpm/pnpm/issues/13827).
