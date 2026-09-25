---
'@pnpm/exec.prepare-package': patch
'@pnpm/fetching.fetcher-base': patch
'@pnpm/fetching.git-fetcher': patch
'@pnpm/fetching.tarball-fetcher': patch
'@pnpm/installing.package-requester': patch
'@pnpm/store.cafs': patch
'@pnpm/store.cafs-types': patch
'@pnpm/worker': patch
'pacquet': patch
'pnpm': patch
---

Enforce `allowBuilds` when a prepared git dependency is reused from the shared store, and use the lockfile's canonical git resolution ID in approval suggestions.
