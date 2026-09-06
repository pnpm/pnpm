---
"pacquet": patch
---

Workspace package patterns such as `./packages/*` now match. Exclusions such as `!./packages/foo` apply too. pnpm normalizes `.` segments, `..` segments, and repeated slashes in the `packages` field of `pnpm-workspace.yaml` before matching them [#14571](https://github.com/pnpm/pnpm/issues/14571).
