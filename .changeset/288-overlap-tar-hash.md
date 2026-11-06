---
"pacquet": patch
---

`pnpm install` unpacks packages with large files faster. pnpm now hashes the whole archive while it writes the package's files to the store. Unpacking `typescript` took 30% less time in our measurements [pnpm/tasks#288](https://github.com/pnpm/tasks/issues/288).
