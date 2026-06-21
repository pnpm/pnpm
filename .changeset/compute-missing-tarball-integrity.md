---
"@pnpm/resolving.npm-resolver": minor
"@pnpm/resolving.resolver-base": minor
"@pnpm/fetching.fetcher-base": minor
"@pnpm/hooks.types": minor
"@pnpm/installing.package-requester": minor
"@pnpm/installing.deps-resolver": patch
"@pnpm/fetching.tarball-fetcher": patch
"@pnpm/fetching.pick-fetcher": patch
"@pnpm/store.controller-types": patch
"@pnpm/lockfile.utils": minor
"@pnpm/deps.graph-builder": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/patching.commands": patch
"pnpm": minor
---

Some registries generate tarballs on-demand and cannot provide an integrity checksum in their package metadata. In that case pnpm now computes the integrity from the downloaded tarball and stores it in the lockfile, so the entry is verifiable on subsequent installs instead of being written without an integrity (which would fail the next install). This also applies to `--lockfile-only`: the tarball is downloaded so its integrity can be computed. A lockfile entry that is still missing its integrity is rejected as a `ERR_PNPM_MISSING_TARBALL_INTEGRITY` lockfile verification violation (the install fails closed) rather than being silently re-fetched.
