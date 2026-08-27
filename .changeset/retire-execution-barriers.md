---
"@pnpm/building.after-install": patch
"@pnpm/building.commands": patch
"@pnpm/building.during-install": patch
"@pnpm/deps.graph-sequencer": major
"@pnpm/exec.lifecycle": patch
"@pnpm/installing.commands": patch
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/releasing.commands": patch
"@pnpm/workspace.projects-sorter": major
"@pnpm/workspace.task-scheduler": major
"pacquet": patch
"pnpm": patch
---

Workspace install, rebuild, pack, publish, stage, and lifecycle work now starts as soon as its dependencies finish instead of waiting for an unrelated topological group.
