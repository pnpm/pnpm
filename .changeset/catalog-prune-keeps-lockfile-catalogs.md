---
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/workspace.workspace-manifest-writer": patch
"pnpm": patch
---

`catalogPrune` no longer removes catalog entries that `pnpm-lock.yaml` still records for workspace projects missing from disk. This affected `pnpm remove` and installs that leave the lockfile unchanged, such as frozen installs. Before, a following frozen install failed with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` [#16679](https://github.com/pnpm/pnpm/issues/16679).
