---
"@pnpm/building.commands": patch
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"pnpm": patch
---

In GVS mode, `pnpm approve-builds` now runs a full install instead of rebuild. This ensures that GVS hash directories and symlinks are updated correctly after changing `allowBuilds`, preventing build artifact contamination of engine-agnostic directories [#11042](https://github.com/pnpm/pnpm/issues/11042).
