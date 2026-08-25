---
"@pnpm/building.commands": patch
"@pnpm/config.writer": patch
"@pnpm/workspace.workspace-manifest-writer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm approve-builds` now removes `onlyBuiltDependencies`, `onlyBuiltDependenciesFile`, `neverBuiltDependencies`, and `ignoredBuiltDependencies` from `pnpm-workspace.yaml` when it writes `allowBuilds`. Those settings were replaced by `allowBuilds` in pnpm 11 and silently ignored since, so a workspace migrated from pnpm 10 kept them around looking active.
