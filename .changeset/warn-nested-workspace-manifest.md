---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now warns when a workspace project has its own `pnpm-workspace.yaml`. The nested file's settings, such as `patchedDependencies`, do not apply when the outer workspace installs that project, because pnpm reads settings only from the `pnpm-workspace.yaml` at the workspace root [#11724](https://github.com/pnpm/pnpm/issues/11724).
