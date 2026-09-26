---
"pnpm": patch
"@pnpm/fetching.directory-fetcher": patch
"@pnpm/fetching.fetcher-base": patch
"@pnpm/installing.client": patch
"@pnpm/releasing.commands": patch
"@pnpm/store.connection-manager": patch
"@pnpm/store.create-cafs-store": patch
"pacquet": patch
---

`pnpm deploy` with a shared lockfile now copies workspace dependencies into the deploy directory, even when `packageImportMethod` is set to `hardlink`. Previously, their files were hard-linked to the workspace sources, so editing a source file also changed the deployed copy [pnpm/pnpm#12176](https://github.com/pnpm/pnpm/issues/12176).
