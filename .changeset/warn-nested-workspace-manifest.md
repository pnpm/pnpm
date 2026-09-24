---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

pnpm now warns when a workspace install covers a project that has its own `pnpm-workspace.yaml`. The nested file's settings, such as `patchedDependencies`, do not apply when the outer workspace installs that project. pnpm reads settings only from the `pnpm-workspace.yaml` at the workspace root [#11724](https://github.com/pnpm/pnpm/issues/11724).
