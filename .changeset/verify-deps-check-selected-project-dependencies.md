---
"@pnpm/deps.status": patch
"pnpm": patch
"pacquet": patch
---

When `verifyDepsBeforeRun` inspects the state a filtered install left behind, it now holds the workspace dependencies of the selected projects to the modules-directory requirement too: the install the gate spawns selects them, so a workspace dependency without a `node_modules` directory leaves the selected projects out of date ([pnpm/tasks#45](https://github.com/pnpm/tasks/issues/45)).
