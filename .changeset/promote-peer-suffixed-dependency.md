---
"pacquet": patch
---

`pnpm install` and `pnpm add` no longer leave a dangling symlink in `node_modules` when a project starts depending directly on a package that the lockfile holds only as a transitive dependency with resolved peer dependencies [#14714](https://github.com/pnpm/pnpm/issues/14714).
