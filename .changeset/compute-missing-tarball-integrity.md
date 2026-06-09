---
"@pnpm/installing.package-requester": minor
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/lockfile.fs": patch
"@pnpm/lockfile.utils": minor
"pnpm": minor
---

When a registry's package metadata does not include an integrity checksum, pnpm now computes the integrity from the downloaded tarball and stores it in the lockfile, instead of writing a lockfile entry without an integrity (which would fail on the next install). If the package is already present in the local store, its tarball is re-downloaded so the integrity can be computed. A non-frozen install will repair a lockfile entry that is missing its integrity; a frozen install cannot edit the lockfile and still fails closed.
