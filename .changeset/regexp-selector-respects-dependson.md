---
"@pnpm/exec.commands": patch
"@pnpm/workspace.task-scheduler": patch
"pnpm": patch
"pacquet": patch
---

`pnpm -r run /regexp/` now honors the `tasks` `dependsOn` declared for each script the selector matches, like running the script by name does. Matched scripts that depend on each other run in order, and each runs once [#15596](https://github.com/pnpm/pnpm/issues/15596).
