---
"@pnpm/installing.linking.hoist": patch
"@pnpm/installing.deps-installer": patch
"pacquet": patch
"pnpm": patch
---

Workspace projects that `hoistPattern` or `publicHoistPattern` selects are now hoisted on every install. A project added to the workspace was not hoisted until `node_modules` was deleted and reinstalled. A workspace that installs nothing from a registry hoisted none of its projects at all [#3642](https://github.com/pnpm/pnpm/issues/3642).
