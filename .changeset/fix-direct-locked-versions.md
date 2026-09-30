---
"pacquet": patch
---

Fixed `pnpm install` changing an unchanged project's direct dependency to a sibling workspace's pinned version when its dependency tree contains a cycle [#16417](https://github.com/pnpm/pnpm/issues/16417).
