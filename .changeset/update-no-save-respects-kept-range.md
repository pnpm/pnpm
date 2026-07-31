---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm update <pkg>@<version>` without saving no longer records a version that the manifest's range excludes. The kept range stays authoritative: a requested version outside it is skipped with a warning instead of producing a lockfile that the next frozen install rejects with `ERR_PNPM_OUTDATED_LOCKFILE` [#12764](https://github.com/pnpm/pnpm/issues/12764).
