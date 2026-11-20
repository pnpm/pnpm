---
"@pnpm/pnpr.client": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/pnpr": patch
"pnpm": patch
"pacquet": patch
---

Installing through a pnpr server now installs a project's peer dependencies when `autoInstallPeers` is enabled. A project that declared only peer dependencies failed with `ERR_PNPM_OUTDATED_LOCKFILE` or skipped its peers [#14833](https://github.com/pnpm/pnpm/issues/14833).
