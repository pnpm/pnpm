---
"@pnpm/config": major
---

`globalDir` is never set. Only the `dir` option is set with the global directory location when the `--global` is used. The pnpm CLI should have access to the global dir, otherwise an exception is thrown.
