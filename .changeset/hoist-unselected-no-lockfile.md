---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

A filtered `pnpm install --no-lockfile` with `hoistWorkspacePackages` now hoists every workspace project into `node_modules/.pnpm/node_modules`, not only the selected ones [pnpm/tasks#168](https://github.com/pnpm/tasks/issues/168).
