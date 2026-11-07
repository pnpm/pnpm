---
"pacquet": patch
---

Fixed installs in a workspace whose `modulesDir` is an absolute path or starts with `..`. A fresh install put the dependencies of workspace projects outside the workspace, and a frozen install linked the root project's dependencies into the wrong directory. pnpm now also removes dependencies you remove from such a directory [pnpm/tasks#285](https://github.com/pnpm/tasks/issues/285).
