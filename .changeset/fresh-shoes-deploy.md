---
"pnpm": patch
"@pnpm/fetching.directory-fetcher": patch
"@pnpm/fetching.fetcher-base": patch
"@pnpm/installing.client": patch
"@pnpm/releasing.commands": patch
"@pnpm/store.connection-manager": patch
---

Fix `pnpm deploy` from shared lockfiles so deployed workspace package dependencies are cloned or copied instead of hard-linked back to the original workspace source files.
