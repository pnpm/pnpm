---
"pacquet": patch
---

`pnpm deploy` now deploys `file:` dependencies that live outside the workspace directory. It failed with "Refusing to deploy unsafe lockfile path" [#16983](https://github.com/pnpm/pnpm/issues/16983).
