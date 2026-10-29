---
"pacquet": patch
---

`pnpm install` now resolves again after an edit to an injected dependency outside the workspace patterns, such as a `workspace:../dir` or `file:../dir` dependency under a shared lockfile. It used to keep the lockfile unchanged, and the next `pnpm install --frozen-lockfile` failed [#17476](https://github.com/pnpm/pnpm/issues/17476).
