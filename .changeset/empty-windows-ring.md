---
"@pnpm/lockfile.verification": minor
pnpm: minor
---

Projects using a `file:` dependency on a local tarball file (i.e. `.tgz`, `.tar.gz`, `.tar`) will see a performance improvement during installation. Previously, using a `file:` dependency on a tarball caused the lockfile resolution step to always run. The lockfile will now be considered up-to-date if the tarball is unchanged.
