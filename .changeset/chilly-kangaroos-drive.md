---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

When running scripts recursively inside a workspace, the logs of the scripts are grouped together in some CI tools. (Only works with `--workspace-concurrency 1`)