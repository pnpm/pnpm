---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

`run --silent <cmd>` should only print output of the command and nothing from pnpm.
