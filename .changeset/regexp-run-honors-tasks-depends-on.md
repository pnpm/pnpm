---
"@pnpm/workspace.task-scheduler": patch
"pnpm": patch
"pacquet": patch
---

`pnpm -r run` with a `/regexp/` script selector now honors the `tasks` `dependsOn` declarations of the scripts it matched, like naming a script directly does. Previously the selector matched no `tasks` entry, so the matched scripts ran without their declared dependencies [#15596](https://github.com/pnpm/pnpm/issues/15596).
