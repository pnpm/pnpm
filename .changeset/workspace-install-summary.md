---
"pacquet": minor
---

In a workspace, the install summary now lists the direct dependency changes of every project, not only of the project pnpm runs in. Each change is listed once, followed by the projects that made it, unless only the current project did. A project's first install lists only deprecated or outdated dependencies and the ones the command added [#6481](https://github.com/pnpm/pnpm/issues/6481).
