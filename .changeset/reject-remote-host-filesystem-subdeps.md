---
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.package-requester": patch
"@pnpm/releasing.commands": patch
"@pnpm/resolving.local-resolver": patch
"@pnpm/store.controller-types": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now rejects local filesystem dependencies (`file:`, `link:`, and path-based `workspace:` specifiers) declared by non-local packages with `ERR_PNPM_LOCAL_SUBDEP_NOT_ALLOWED`. Relative `file:` dependencies inside a package's own archive and local dependencies configured by the workspace through `overrides`, `packageExtensions`, or `.pnpmfile.cjs` continue to work. Lockfile verification also rejects unauthorized local directory, tarball, and link entries on non-importer snapshots.
