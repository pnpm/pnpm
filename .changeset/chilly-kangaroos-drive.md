---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

When running scripts recursively inside a workspace, the logs of the scripts are grouped together in the Github Actions UI. (Only works with `--workspace-concurrency 1`)