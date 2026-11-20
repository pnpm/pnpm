---
"@pnpm/installing.deps-installer": patch
"@pnpm/pnpr.client": patch
"pnpm": patch
"pacquet": patch
"@pnpm/pnpr": patch
---

Fixed `pnpm` installs using pnpr to honor the client's `autoInstallPeers`, `dedupePeers`, and `excludeLinksFromLockfile` settings [pnpm/pnpm#13389](https://github.com/pnpm/pnpm/issues/13389).
