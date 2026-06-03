---
"pnpm": patch
"@pnpm/fetching.directory-fetcher": patch
"@pnpm/fetching.fetcher-base": patch
"@pnpm/installing.client": patch
"@pnpm/releasing.commands": patch
"@pnpm/store.connection-manager": patch
---

Deploying a project from a shared lockfile now clones or copies workspace package dependencies into the deployed directory. Previously, workspace dependency files were hard-linked to the source packages [pnpm/pnpm#12176](https://github.com/pnpm/pnpm/issues/12176).
