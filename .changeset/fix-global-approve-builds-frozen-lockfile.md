---
"@pnpm/building.commands": patch
"pnpm": patch
---

Fix `ERR_PNPM_OUTDATED_LOCKFILE` when approving builds during a global install. The `approve-builds` flow called by `pnpm add -g` passed the global packages directory to the subsequent install as `workspaceDir`, which caused sibling install directories (such as those left behind by `pnpm self-update`) to be picked up as workspace projects and fail the frozen-lockfile check.
