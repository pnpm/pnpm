---
"pacquet": patch
---

Fixed `link:` dependencies of the root project when `modulesDir` is an absolute path that goes through a symlink, such as a directory under `/var` or `/tmp` on macOS. pnpm created their links in that directory but they pointed nowhere, so their commands were missing [pnpm/tasks#285](https://github.com/pnpm/tasks/issues/285).
