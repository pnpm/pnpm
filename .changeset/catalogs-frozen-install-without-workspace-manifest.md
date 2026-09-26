---
"@pnpm/config.reader": patch
"@pnpm/lockfile.settings-checker": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

Fixed a false `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` on frozen installs in directories that have a lockfile with catalogs but no `pnpm-workspace.yaml` (a pruned production artifact, for instance). When no workspace manifest supplies the catalog configuration, the catalogs recorded in the lockfile are the only ones there are, so they are no longer compared against an empty configuration. A workspace manifest that exists but drops a catalog entry still fails the frozen install.
