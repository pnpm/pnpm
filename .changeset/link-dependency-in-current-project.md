---
"@pnpm/installing.commands": patch
"pnpm": patch
---

In a workspace, `pnpm link <dir>` now adds the `link:` dependency to the `package.json` of the project where the command runs, not to the workspace root's `package.json`. The link is created in that project's `node_modules`, as documented. The override is still written to the root `pnpm-workspace.yaml`.
