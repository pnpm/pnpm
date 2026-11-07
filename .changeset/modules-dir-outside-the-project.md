---
"pacquet": patch
---

Fixed installs in a workspace whose `modulesDir` is an absolute path or starts with `..`. A fresh install no longer puts the dependencies of workspace projects outside the workspace. A frozen install now links the root project's dependencies into the configured directory. pnpm now also removes a dependency you remove from such a directory. With `excludeLinksFromLockfile`, the root project's linked dependencies now go to the configured directory. The root project's lifecycle scripts now find the commands of its dependencies. The `loaded` node linker now puts the root project's commands in that directory's `.bin`. `pnpm bin`, `pnpm root`, `pnpm run`, `pnpm exec`, and `pnpm version` now look for a project's commands where the install puts them [pnpm/tasks#285](https://github.com/pnpm/tasks/issues/285).
