---
"@pnpm/fetching.tarball-fetcher": patch
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/lockfile.types": patch
"@pnpm/lockfile.utils": patch
"@pnpm/pnpr.client": patch
"@pnpm/resolving.npm-resolver": patch
"@pnpm/resolving.registry.types": patch
"@pnpm/resolving.resolver-base": patch
"@pnpm/resolving.tarball-url": patch
"@pnpm/store.controller-types": patch
"pnpm": patch
---

Registry revisions are no longer supported. pnpm now fails with `ERR_PNPM_REGISTRY_REVISIONS_UNSUPPORTED` on registry metadata that selects a revision, on a `+rN` version specifier, and on a lockfile entry with a `revision` field. `pnpm update --patches` is removed. Registry revisions are supported by pnpm v12.
