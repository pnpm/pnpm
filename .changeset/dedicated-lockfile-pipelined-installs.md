---
"pacquet": patch
---

In a workspace with `sharedWorkspaceLockfile: false`, `pnpm install` no longer waits for the workspace projects a project depends on before it starts installing that project. Each project is resolved, fetched, and written to its virtual store right away, up to `workspaceConcurrency` at a time. A project waits for its workspace dependencies only before it links its dependencies and runs its lifecycle scripts, so those scripts still run after the scripts of the projects it depends on. A project with a `preinstall` script, or with an injected or `file:` workspace dependency, still waits for its dependencies before it starts [#14480](https://github.com/pnpm/pnpm/issues/14480).
