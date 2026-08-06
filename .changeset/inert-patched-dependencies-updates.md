---
"@pnpm/installing.deps-installer": minor
"@pnpm/patching.config": minor
"pacquet": patch
"pnpm": minor
---

Changing `patchedDependencies` no longer re-resolves the dependency graph when the change targets no package the lockfile records. A patch that matches a locked package still goes through a full resolution, because applying it rekeys that package's entry. The install falls back to a full resolution whenever the new configuration would leave a patch unused and `allowUnusedPatches` is off, so `ERR_PNPM_UNUSED_PATCH` is still reported.
