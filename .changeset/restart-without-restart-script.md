---
"@pnpm/exec.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm restart` now runs the "stop" and "start" scripts when the package has no "restart" script. Previously it ran "stop" and then failed with "Missing script: restart" [#4750](https://github.com/pnpm/pnpm/issues/4750).
