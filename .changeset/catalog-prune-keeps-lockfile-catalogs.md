---
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/workspace.workspace-manifest-writer": patch
"pnpm": patch
---

`catalogPrune` no longer removes catalog entries from `pnpm-workspace.yaml` that `pnpm-lock.yaml` still records. This affected `pnpm remove` and installs that leave the lockfile unchanged, such as frozen installs. A following frozen install then failed with `ERR_PNPM_LOCKFILE_CONFIG_MISMATCH` [#16679](https://github.com/pnpm/pnpm/issues/16679).
