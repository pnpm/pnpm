---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

`pnpm run` should fail if the path to the project contains colon(s).
