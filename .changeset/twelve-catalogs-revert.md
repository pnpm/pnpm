---
"@pnpm/catalogs.config": patch
"@pnpm/installing.commands": patch
"pnpm": patch
---

Fix `pnpm install` reporting "Already up to date" after a catalog entry in `pnpm-workspace.yaml` was reverted to a previous version. After an update modified a catalog, the workspace state cache stored the pre-update catalog versions, so reverting the entry back to its original version was not detected as an outdated state [#12418](https://github.com/pnpm/pnpm/issues/12418).
