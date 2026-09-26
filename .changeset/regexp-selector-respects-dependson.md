---
"@pnpm/exec.commands": patch
"@pnpm/workspace.task-scheduler": patch
"pnpm": patch
"pacquet": patch
---

`pnpm -r run` with a `/regexp/` script selector now runs the `dependsOn` tasks declared for the scripts the selector matches. Running the script by name already did this [#15596](https://github.com/pnpm/pnpm/issues/15596).
