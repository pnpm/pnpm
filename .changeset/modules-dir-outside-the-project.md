---
"pacquet": patch
---

Fixed installs in a workspace whose `modulesDir` is an absolute path or starts with `..`. A fresh install no longer puts the dependencies of workspace projects outside the workspace. A frozen install now links the root project's dependencies into the configured directory. pnpm now also removes a dependency you remove from such a directory [pnpm/tasks#285](https://github.com/pnpm/tasks/issues/285).
