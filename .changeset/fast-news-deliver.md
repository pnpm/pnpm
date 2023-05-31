---
"@pnpm/lockfile-types": minor
"@pnpm/lockfile-file": minor
"@pnpm/core": minor
"pnpm": minor
---

Some settings influence the structure of the lockfile, so we cannot reuse the lockfile if those settings change. As a result, we need to store such settings in the lockfile. This way we will know with which settings the lockfile has been created.

A new field will now be present in the lockfile: `settings`. It will store the values of two settings: `autoInstallPeers` and `excludeLinksFromLockfile`. If someone tries to perform a `frozen-lockfile` installation and their active settings don't match the ones in the lockfile, then an error message will be thrown.

The lockfile format version is bumped from v6.0 to v6.1.

Related PR: [#6557](https://github.com/pnpm/pnpm/pull/6557)
Related issue: [#6312](https://github.com/pnpm/pnpm/issues/6312)
