---
"@pnpm/lockfile.verification": patch
pnpm: patch
---

Fix a bug causing `pnpm install` to incorrectly assume the lockfile is up to date after changing a local tarball that has peers dependencies.
