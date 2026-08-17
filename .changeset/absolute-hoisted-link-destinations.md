---
"@pnpm/building.during-install": patch
"pnpm": patch
---

Fixed intermittent `ERR_PNPM_ENOENT` and `ERR_PNPM_ENOTEMPTY` errors while renaming `_tmp_*` directories during installation with `nodeLinker: hoisted`, in workspaces that also use `patchedDependencies`.
