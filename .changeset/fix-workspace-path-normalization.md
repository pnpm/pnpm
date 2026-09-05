---
"pacquet": patch
---

Workspace package patterns now normalize `.` and `..` segments and repeated slashes before matching. Patterns such as `./packages/*` discover packages, and exclusions such as `!./packages/foo` exclude them correctly [#14571](https://github.com/pnpm/pnpm/issues/14571).
